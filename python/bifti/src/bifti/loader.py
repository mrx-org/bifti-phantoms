# Reference loader for NIfTI phantoms: turns a phantom (parsed by nifti_phantom)
# into plain NumPy arrays. A readable example for porting to your own library -
# not optimised or feature-complete. Deps: numpy, nibabel (and torch, used for
# resampling when it is installed). See ../SPEC.md for the format and DEMO.md
# for usage.

from __future__ import annotations

import copy
import re
from dataclasses import dataclass
from functools import lru_cache
from pathlib import Path

import numpy as np
import nibabel

from ._nifti_write import write_stack
from .resample import DEFAULT_MAX_SUBSTEPS, Resampler, density_weight, warn_grid_mismatch
from .phantom import (
    BiftiPhantom,
    BiftiTissue,
    NiftiRef,
    NiftiMapping,
    Patient,
    ResliceTo,
    patient_to_scanner,
)


# ===========================================================================
# Public entry points
# ===========================================================================


@dataclass
class NumpyPhantom:
    """A loaded bifti phantom + maps, represented as dict of [`NumpyTissue`]"""

    config: BiftiPhantom
    tissues: dict[str, NumpyTissue]

    @classmethod
    def load(cls, path: Path | str, *, max_substeps: int = DEFAULT_MAX_SUBSTEPS):
        """Load a complete phantom from its ``phantom.json``.

        NIfTI files are resolved relative to the JSON file's directory, matching the
        folder convention in ``../SPEC.md``.

        ``max_substeps`` caps the samples per axis of each output voxel when an oblique
        ``reslice_to`` falls back to quadrature (see :mod:`bifti.resample`); raise it when
        the warning says the quadrature is under-sampled. Axis-aligned grids ignore it.
        """
        path = Path(path)
        config = BiftiPhantom.load(Path(path))
        return cls.load_from_config(config, base_dir=path.parent, max_substeps=max_substeps)

    @classmethod
    def load_from_config(
        cls,
        config: BiftiPhantom,
        base_dir: Path | str,
        *,
        max_substeps: int = DEFAULT_MAX_SUBSTEPS,
    ):
        """Load all tissues of an already-parsed config from ``base_dir``.

        Useful if you want to tweak the config in memory before loading the data.
        ``max_substeps`` is as in :meth:`load`.
        """
        return cls(
            config=config,
            tissues={
                name: load_tissue(
                    tissue, Path(base_dir), config.reslice_to, max_substeps=max_substeps
                )
                for name, tissue in config.tissues.items()
            },
        )

    def save(
        self,
        out_dir: Path | str,
        json_name: str = "phantom.json",
        *,
        constant_threshold: float | None = None,
    ) -> Path:
        """Save the phantom as a self-contained directory and return the JSON's path.

        This writes what :meth:`load` produced, so loading with a ``reslice_to`` and then
        saving is how a phantom is resliced on disk. The saved phantom has no
        ``reslice_to`` and **no** ``func`` **mappings**: every mapping was already applied
        to the resampled data and its reference becomes a plain one.

        Layout: one gzip-compressed float64 (complex128 for complex maps) NIfTI per
        property, named after the JSON (``phantom_density.nii.gz``, ``phantom_T1.nii.gz``,
        ``phantom_B1plus_0.nii.gz``, ... for ``phantom.json``) so several phantoms can
        share a directory, with the tissues stacked along the 4th dimension in order of
        their sorted names. Properties that are constants in the phantom stay constants in
        the JSON. The output is deterministic: the same phantom always gives the same bytes.

        ``constant_threshold`` also stores a map as a constant when it is (nearly) one:
        when its RMS deviation from the real part of its mean - its standard deviation,
        for a real map - is at most the threshold, that mean replaces it. ``0`` collapses
        exactly constant maps only; ``None`` (the default) or a negative value keeps every
        map. ``density`` is always a map.

        .. warning::
           The result can be considerably **larger than the input**. Tissues that
           referenced the same map (with or without different ``func``) each get their own
           copy of it, unless the copies are bit-identical, in which case they share one
           volume. When reslicing, intensive properties are weighted by each tissue's own
           density, so copies of one source map usually differ between tissues. Data is
           stored as float64 (or complex128) whatever the type of the source NIfTIs.
        """
        out_dir = Path(out_dir)
        out_dir.mkdir(parents=True, exist_ok=True)

        config = copy.deepcopy(self.config)
        config.reslice_to = None
        names = sorted(config.tissues)
        missing = [n for n in names if n not in self.tissues]
        if missing:
            raise ValueError(f"tissues {missing} are in the config but were not loaded")
        config.tissues = {name: config.tissues[name] for name in names}
        prefix = Path(json_name).stem

        for prop in _PROPERTIES:
            n_channels = max(
                (prop.n_channels(config.tissues[n]) for n in names), default=0
            )
            for channel in range(n_channels):
                # Constants stay in the JSON; only maps get a NIfTI.
                members = []
                for n in names:
                    if not prop.is_map(config.tissues[n], channel):
                        continue
                    value = None
                    if prop.attr != "density":
                        value = _as_constant(
                            prop.volume(self.tissues[n], channel), constant_threshold
                        )
                    if value is None:
                        members.append(n)
                    else:
                        prop.set(config.tissues[n], channel, value)
                if not members:
                    continue
                affines = {
                    tuple(map(tuple, self.tissues[n].affine)) for n in members
                }
                if len(affines) > 1:
                    raise ValueError(
                        f"{prop.stem(channel)}: tissues have different affines, "
                        "cannot stack them into one NIfTI"
                    )
                file_name = Path(f"{prefix}_{prop.stem(channel)}.nii.gz")
                slots = write_stack(
                    out_dir / file_name,
                    [prop.volume(self.tissues[n], channel) for n in members],
                    self.tissues[members[0]].affine,
                )
                for name, slot in zip(members, slots):
                    prop.set(config.tissues[name], channel, NiftiRef(file_name, slot))

        json_path = out_dir / json_name
        config.save(json_path)
        return json_path

    def scanner_affine(self, tissue: str) -> np.ndarray:
        """The 4x4 voxel -> scanner affine of one tissue.

        A tissue's own ``affine`` maps voxels into the subject-aligned RAS+
        space the phantom is stored in; this composes it with the phantom's
        patient position to land in scanner coordinates instead. Identical to
        ``affine`` when the phantom carries no ``patient``
        (see ``../NIFTI.md#patient-position``).
        """
        return to_scanner_affine(self.tissues[tissue].affine, self.config.patient)


def to_scanner_affine(
    affine: list[list[float]] | np.ndarray, patient: Patient | None
) -> np.ndarray:
    """Compose a voxel -> RAS+ affine with a patient position.

    ``affine`` is the upper 3 rows (as stored in a phantom) or a full 4x4;
    the result is always 4x4. ``patient`` may be ``None``, which is FFS and
    leaves the affine untouched (../NIFTI.md#patient-position).
    """
    affine = np.asarray(affine, dtype=np.float64)
    if affine.shape == (3, 4):
        affine = np.vstack([affine, [0.0, 0.0, 0.0, 1.0]])
    if affine.shape != (4, 4):
        raise ValueError(f"Expected a 3x4 or 4x4 affine, got {affine.shape}")

    rotation = np.eye(4)
    rotation[:3, :3] = patient_to_scanner(patient.position if patient else None)
    return rotation @ affine


@dataclass
class NumpyTissue:
    """A single tissue with every property resolved to a NumPy array.

    All scalar properties are expanded to full arrays so that downstream code
    can treat uniform and spatially-varying tissues the same way. Every array
    shares the same 3D ``shape`` and the same voxel-to-world ``affine``.

    Units follow the spec (see ``../JSON.md``): T1/T2/T2' in seconds, ADC in
    1e-3 mm^2/s, dB0 in Hz, B1+/B1- relative, density a volume fraction.
    """

    density: np.ndarray  # (X, Y, Z)            volume fraction
    T1: np.ndarray  #      (X, Y, Z)            seconds
    T2: np.ndarray  #      (X, Y, Z)            seconds
    T2dash: np.ndarray  #  (X, Y, Z)            seconds
    ADC: np.ndarray  #     (X, Y, Z)            1e-3 mm^2/s
    dB0: np.ndarray  #     (X, Y, Z)            Hz
    B1_tx: np.ndarray  #   (channels, X, Y, Z)  relative transmit field, maybe complex
    B1_rx: np.ndarray  #   (channels, X, Y, Z)  relative receive field, maybe complex
    resliced: ResliceTo  # Affine + resolution this phantom is resliced to

    @property
    def shape(self) -> list[int]:
        return self.resliced.resolution

    @property
    def affine(self) -> list[list[float]]:
        return self.resliced.affine


# ===========================================================================
# Saving: which per-tissue maps a phantom has, and how they map to files
# ===========================================================================


@dataclass(frozen=True)
class _Property:
    """One per-tissue map of a phantom, as :meth:`NumpyPhantom.save` groups them into files.

    ``attr`` names the field on both :class:`BiftiTissue` (the reference) and
    :class:`NumpyTissue` (the data); ``channels`` marks the ``B1+``/``B1-`` lists.
    """

    attr: str
    file_stem: str
    channels: bool = False

    def stem(self, channel: int) -> str:
        return f"{self.file_stem}_{channel}" if self.channels else self.file_stem

    def n_channels(self, tissue: BiftiTissue) -> int:
        return len(getattr(tissue, self.attr)) if self.channels else 1

    def _config(self, tissue: BiftiTissue, channel: int):
        value = getattr(tissue, self.attr)
        if not self.channels:
            return value
        return value[channel] if channel < len(value) else None

    def is_map(self, tissue: BiftiTissue, channel: int) -> bool:
        if self.attr == "density":
            return True
        return isinstance(self._config(tissue, channel), (NiftiRef, NiftiMapping))

    def volume(self, tissue: NumpyTissue, channel: int) -> np.ndarray:
        data = getattr(tissue, self.attr)
        return data[channel] if self.channels else data

    def set(self, tissue: BiftiTissue, channel: int, value: NiftiRef | float) -> None:
        if self.channels:
            getattr(tissue, self.attr)[channel] = value
        else:
            setattr(tissue, self.attr, value)


def _as_constant(volume: np.ndarray, threshold: float | None) -> float | None:
    """The real constant ``volume`` can be stored as, or ``None`` if it must stay a map.

    That is the real part of its mean, if the RMS deviation from it is at most
    ``threshold``. An exactly constant real volume gives its value even at threshold 0.
    """
    if threshold is None or threshold < 0 or volume.size == 0:
        return None
    first = volume.flat[0]
    if np.imag(first) == 0 and np.all(volume == first):
        return float(np.real(first))
    value = float(np.real(volume.mean()))
    deviation = np.sqrt(np.mean(np.abs(volume - value) ** 2))
    return value if deviation <= threshold else None


_PROPERTIES = (
    _Property("density", "density"),
    _Property("T1", "T1"),
    _Property("T2", "T2"),
    _Property("T2dash", "T2dash"),
    _Property("ADC", "ADC"),
    _Property("dB0", "dB0"),
    _Property("B1_tx", "B1plus", channels=True),
    _Property("B1_rx", "B1minus", channels=True),
)


# ===========================================================================
# Phantom loading internals
# ===========================================================================


def load_tissue(
    tissue: BiftiTissue,
    base_dir: Path | str,
    reslice_to: ResliceTo | None = None,
    *,
    max_substeps: int = DEFAULT_MAX_SUBSTEPS,
) -> NumpyTissue:
    """Load one tissue, resolving every property to a NumPy array.

    The output grid is ``reslice_to`` if given, otherwise the density map's own grid - so
    every other map is brought onto the density resolution and affine (see ``../JSON.md``
    -> ``reslice_to``). The spec requires a phantom's NIfTIs to already share that grid, so
    for conforming data the implicit resampling is a no-op.

    Resampling averages each output voxel over the source region it covers, weighted by
    this tissue's ``density`` - see :mod:`bifti.resample` for why the weighting matters.
    """
    base_dir = Path(base_dir)

    # The density map defines the source grid *and* supplies the resampling weight, so it
    # is loaded natively first and resampled separately from everything else.
    density_src, src_affine = load_file_ref_noreslice(base_dir, tissue.density)
    _real_valued(density_src, "density")
    src_shape = tuple(density_src.shape)

    if reslice_to is None:
        reslice_to = ResliceTo(affine=src_affine, resolution=list(src_shape))

    resampler = Resampler.build(
        src_affine,
        src_shape,
        reslice_to.affine,
        reslice_to.resolution,
        max_substeps=max_substeps,
    )

    # `density` is a volume fraction - extensive - so it is averaged *unweighted*.
    # Weighting it by itself would renormalise away exactly the partial-volume information
    # the resampling is meant to produce.
    density = resampler.resample_plain(density_src)

    # The denominator every intensive property of this tissue shares.
    weight = density_weight(density_src)
    weight_sum = resampler.weight_sum(weight)

    def prop(cfg, name) -> np.ndarray:
        return load_property(
            cfg, base_dir, reslice_to, resampler, weight, weight_sum, src_affine,
            src_shape, name, max_substeps,
        )

    def channels(cfgs, name) -> np.ndarray:
        return np.stack(
            [prop(ch, f"{name}[{i}]") for i, ch in enumerate(cfgs)], axis=0
        )

    return NumpyTissue(
        density=density,
        T1=_real_valued(prop(tissue.T1, "T1"), "T1"),
        T2=_real_valued(prop(tissue.T2, "T2"), "T2"),
        T2dash=_real_valued(prop(tissue.T2dash, "T2'"), "T2'"),
        ADC=_real_valued(prop(tissue.ADC, "ADC"), "ADC"),
        dB0=_real_valued(prop(tissue.dB0, "dB0"), "dB0"),
        B1_tx=channels(tissue.B1_tx, "B1+"),
        B1_rx=channels(tissue.B1_rx, "B1-"),
        resliced=reslice_to,
    )


def load_property(
    config: float | NiftiRef | NiftiMapping,
    base_dir: Path,
    reslice_to: ResliceTo,
    resampler: Resampler,
    weight: np.ndarray,
    weight_sum: np.ndarray,
    src_affine: list[list[float]],
    src_shape: tuple[int, ...],
    name: str,
    max_substeps: int,
) -> np.ndarray:
    """Resolve one "scalar-or-map" property to a 3D array (../JSON.md).

    1. a number      -> a uniform array of ``shape`` filled with that value;
    2. a NIfTI ref   -> the referenced sub-volume, density-weighted onto the target grid;
    3. a transformed -> case 2 with ``func`` applied per voxel.

    Maps come back already on the output grid, so a ``func`` and its ``x_*`` statistics act
    on the resampled values.
    """
    if isinstance(config, (int, float)):
        # A uniform value needs no resampling - just fill the target grid.
        return np.full(reslice_to.resolution, float(config), dtype=np.float64)
    if isinstance(config, NiftiRef):
        ref = config
    elif isinstance(config, NiftiMapping):
        ref = config.file
    else:
        raise TypeError(
            f"property must be a number, NiftiRef or NiftiMapping, got {type(config)}"
        )

    native, native_affine = load_file_ref_noreslice(base_dir, ref)
    if tuple(native.shape) == src_shape and np.allclose(native_affine, src_affine):
        data = resampler.resample_weighted(native, weight, weight_sum)
    else:
        # ../NIFTI.md requires every NIfTI of a phantom to share a grid. A non-conforming
        # file cannot use the density weight, so fall back to an unweighted average.
        warn_grid_mismatch(name)
        other = Resampler.build(
            native_affine,
            native.shape,
            reslice_to.affine,
            reslice_to.resolution,
            max_substeps=max_substeps,
        )
        data = other.resample_plain(native)

    if isinstance(config, NiftiMapping):
        return eval_expr(config.func, data)
    return data


# ===========================================================================
# NIfTI file access
# ===========================================================================


def load_file_ref_noreslice(
    base_dir: Path, ref: NiftiRef
) -> tuple[np.ndarray, list[list[float]]]:
    """Load the sub-volume named by ``ref`` on its native grid.

    ``ref`` is a ``"<file>[<index>]"`` reference; ``index`` selects along the
    NIfTI's 4th (tissue) dimension. Returns the 3D sub-volume and the file's own
    3x4 affine (the upper rows of its 4x4 voxel-to-world transform).
    """
    data, affine = _load_nifti(base_dir, ref.file_name)
    return data[:, :, :, ref.tissue_index], affine


def _real_valued(data: np.ndarray, name: str) -> np.ndarray:
    if np.iscomplexobj(data):
        raise ValueError(f"{name} must be real-valued, got {data.dtype} data")
    return data


# Avoid re-loading NIfTIs for every tissue by caching the native (unresampled) file.
# Resampling can no longer be cached per file: it depends on the *tissue's* density map,
# which generally lives in a different file. Caching the raw load instead is if anything a
# stronger cache, since it no longer varies with the target grid.
@lru_cache(maxsize=20)
def _load_nifti(
    base_dir: Path, file_name: Path
) -> tuple[np.ndarray, list[list[float]]]:
    # A reference may be relative to the phantom directory or an absolute path.
    path = file_name if file_name.is_absolute() else (base_dir / file_name).resolve()
    img = nibabel.load(path)
    assert isinstance(img, nibabel.Nifti1Image)
    assert len(img.shape) == 4

    data = np.asarray(img.dataobj)
    data = data.astype(np.complex128 if np.iscomplexobj(data) else np.float64, copy=False)
    sform = img.get_sform()  # full 4x4 voxel-to-world (RAS+, mm); see ../NIFTI.md
    return data, sform[:3].tolist()


# ===========================================================================
# `func` transforms (../JSON.md -> "Transformed reference")
# ===========================================================================


# The grammar is parsed exactly - never `eval`ed - so a phantom file cannot run code:
#
#     expr   = term   (("+" | "-") term)*
#     term   = factor (("*" | "/") factor)*
#     factor = number | variable | "(" expr ")" | ("+" | "-") factor

FUNC_VARIABLES = ("x", "x_min", "x_max", "x_mean", "x_std")
FUNC_OPERATORS = {"+": np.add, "-": np.subtract, "*": np.multiply, "/": np.divide}
FUNC_TOKEN = re.compile(
    r"\s*(?:(?P<number>(?:\d+\.?\d*|\.\d+)(?:[eE][+-]?\d+)?)"
    r"|(?P<name>[A-Za-z_]\w*)|(?P<symbol>[-+*/()]))"
)


def _func_error(func: str, msg: str) -> ValueError:
    return ValueError(f"Invalid func {func!r}: {msg}")


def _tokenize(func: str) -> list[float | str]:
    """Numbers become floats, variables and symbols stay strings."""
    tokens: list[float | str] = []
    pos = 0
    while func[pos:].strip():
        m = FUNC_TOKEN.match(func, pos)
        if m is None:
            raise _func_error(func, f"unexpected character at position {pos}")
        if m["number"] is not None:
            tokens.append(float(m["number"]))
        elif m["name"] is not None:
            if m["name"] not in FUNC_VARIABLES:
                raise _func_error(func, f"unknown variable {m['name']}")
            tokens.append(m["name"])
        else:
            tokens.append(m["symbol"])
        pos = m.end()
    return tokens


class _FuncParser:
    def __init__(self, func: str):
        self.func = func
        self.tokens = _tokenize(func)
        self.pos = 0

    def current(self) -> float | str | None:
        return self.tokens[self.pos] if self.pos < len(self.tokens) else None

    def next(self) -> float | str | None:
        token = self.current()
        self.pos += 1
        return token

    def binary(self, operand, ops: tuple[str, ...]):
        lhs = operand()
        while self.current() in ops:
            op = FUNC_OPERATORS[self.next()]
            lhs = (op, lhs, operand())
        return lhs

    def expr(self):
        return self.binary(self.term, ("+", "-"))

    def term(self):
        return self.binary(self.factor, ("*", "/"))

    def factor(self):
        token = self.next()
        if isinstance(token, float) or token in FUNC_VARIABLES:
            return token
        if token == "(":
            expr = self.expr()
            if self.next() != ")":
                raise _func_error(self.func, "expected ')'")
            return expr
        if token in ("+", "-"):
            return (FUNC_OPERATORS[token], 0.0, self.factor())
        raise _func_error(
            self.func, "unexpected end" if token is None else f"unexpected {token!r}"
        )


def parse_func(func: str):
    """Parse a ``func`` expression into a nested ``(operator, lhs, rhs)`` tree of
    numbers and variable names. Raises ``ValueError`` on anything outside the grammar.
    """
    parser = _FuncParser(func)
    ast = parser.expr()
    if parser.current() is not None:
        raise _func_error(func, f"unexpected {parser.current()!r}")
    return ast


def _evaluate(ast, variables: dict):
    if isinstance(ast, float):
        return ast
    if isinstance(ast, str):
        return variables[ast]
    op, lhs, rhs = ast
    return op(_evaluate(lhs, variables), _evaluate(rhs, variables))


def eval_expr(func: str, data: np.ndarray) -> np.ndarray:
    """Apply a ``func`` transform to a voxel array.

    ``x`` is the per-voxel value; ``x_min``/``x_max``/``x_mean``/``x_std`` are
    scalar statistics of the whole volume (``x_min``/``x_max`` compare real parts).
    """
    ast = parse_func(func)
    variables = {
        "x": data,
        "x_min": np.real(data).min(),
        "x_max": np.real(data).max(),
        "x_mean": data.mean(),
        "x_std": data.std(),
    }
    # An expression without x is a constant, but the map stays a map.
    return np.broadcast_to(_evaluate(ast, variables), data.shape).copy()


# ===========================================================================
# Example usage
# ===========================================================================

if __name__ == "__main__":
    import sys

    if len(sys.argv) != 2:
        print("usage: python nifti_loader.py <path/to/phantom.json>")
        raise SystemExit(2)

    phantom = NumpyPhantom.load(sys.argv[1])
    for name, tissue in phantom.tissues.items():
        print(f"{name}: shape={tissue.shape}, B1+ channels={tissue.B1_tx.shape[0]}")
        print(
            f"    T1 mean={np.nanmean(tissue.T1):.4g} s, dB0 range="
            f"[{tissue.dB0.min():.4g}, {tissue.dB0.max():.4g}] Hz"
        )

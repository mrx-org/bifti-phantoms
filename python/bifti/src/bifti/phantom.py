from dataclasses import dataclass
from typing import Literal, Any
from pathlib import Path
import warnings


def warn_unknown_fields(where: str, config: dict[str, Any], known: set[str]):
    """Warn about fields we don't know, per ../SPEC.md.

    The format is additively extensible, so unknown fields must be ignored
    rather than rejected - but since the schema no longer catches typos, the
    reader is the only place left that can point one out.
    """
    for key in config:
        if key not in known:
            warnings.warn(f"Ignoring unknown field {key!r} in {where}", stacklevel=3)


# DICOM-style patient position codes and the rotation from phantom RAS+ to
# scanner coordinates each one defines: `v_scanner = P @ v_ras`. The scanner
# frame is right-handed with Z along B0 pointing out of the bore and Y pointing
# up, which makes FFS the identity. See ../NIFTI.md#patient-position.
PATIENT_POSITIONS: dict[str, list[list[float]]] = {
    # feet first
    "FFS": [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
    "FFP": [[-1.0, 0.0, 0.0], [0.0, -1.0, 0.0], [0.0, 0.0, 1.0]],
    "FFDR": [[0.0, 1.0, 0.0], [-1.0, 0.0, 0.0], [0.0, 0.0, 1.0]],
    "FFDL": [[0.0, -1.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]],
    # head first
    "HFS": [[-1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, -1.0]],
    "HFP": [[1.0, 0.0, 0.0], [0.0, -1.0, 0.0], [0.0, 0.0, -1.0]],
    "HFDR": [[0.0, -1.0, 0.0], [-1.0, 0.0, 0.0], [0.0, 0.0, -1.0]],
    "HFDL": [[0.0, 1.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, -1.0]],
}

# A phantom without a patient position is not transformed at all, which is the
# same as saying it is feet first supine.
DEFAULT_PATIENT_POSITION = "FFS"


def patient_to_scanner(position: str | None) -> list[list[float]]:
    """The 3x3 phantom-RAS+ -> scanner rotation of a patient position.

    ``None`` (no ``patient`` in the phantom) yields the identity, so callers
    don't have to special-case the default anywhere else.
    """
    if position is None:
        position = DEFAULT_PATIENT_POSITION
    if position not in PATIENT_POSITIONS:
        raise ValueError(
            f"Unknown patient position {position!r}, "
            f"expected one of {sorted(PATIENT_POSITIONS)}"
        )
    return [row.copy() for row in PATIENT_POSITIONS[position]]


@dataclass
class PhantomUnits:
    gyro: Literal["MHz/T"]
    B0: Literal["T"]
    T1: Literal["s"]
    T2: Literal["s"]
    T2dash: Literal["s"]
    ADC: Literal["10^-3 mm^2/s"]
    dB0: Literal["Hz"]
    B1_tx: Literal["rel"]
    B1_rx: Literal["rel"]

    @classmethod
    def default(cls):
        return cls(
            gyro="MHz/T",
            B0="T",
            T1="s",
            T2="s",
            T2dash="s",
            ADC="10^-3 mm^2/s",
            dB0="Hz",
            B1_tx="rel",
            B1_rx="rel",
        )

    @classmethod
    def from_dict(cls, config: dict[str, str]):
        # Currently this is only for documentation and no other units than the
        # default units are supported. This definetly can change in the future
        # but the implementation has no priority right now
        default = cls.default()
        assert default.to_dict() == config, "Only default units are supported for now"
        return default

    def to_dict(self) -> dict[str, str]:
        return {
            "gyro": self.gyro,
            "B0": self.B0,
            "T1": self.T1,
            "T2": self.T2,
            "T2'": self.T2dash,
            "ADC": self.ADC,
            "dB0": self.dB0,
            "B1+": self.B1_tx,
            "B1-": self.B1_rx,
        }


@dataclass
class PhantomSystem:
    gyro: float
    B0: float

    @classmethod
    def default(cls):
        return cls(
            gyro=42.5764,
            B0=3.0
        )

    @classmethod
    def from_dict(cls, config: dict[str, float]):
        warn_unknown_fields("system", config, {"gyro", "B0"})
        return cls(gyro=config["gyro"], B0=config["B0"])

    def to_dict(self) -> dict[str, float]:
        return {"gyro": self.gyro, "B0": self.B0}


@dataclass
class NiftiRef:
    file_name: Path
    tissue_index: int

    @classmethod
    def parse(cls, config: str):
        import re

        regex = re.compile(r"(?P<file>.+?)\[(?P<idx>\d+)\]$")
        m = regex.match(config)
        if not m:
            raise ValueError("Invalid file_ref", m)
        return cls(file_name=Path(m.group("file")), tissue_index=int(m.group("idx")))

    def to_str(self) -> str:
        return f"{self.file_name}[{self.tissue_index}]"


@dataclass
class NiftiMapping:
    file: NiftiRef
    func: str

    @classmethod
    def parse(cls, config: dict[str, Any]):
        warn_unknown_fields("a transformed reference", config, {"file", "func"})
        return cls(file=NiftiRef.parse(config["file"]), func=config["func"])

    def to_dict(self) -> dict[str, Any]:
        return {"file": self.file.to_str(), "func": self.func}


@dataclass
class BiftiTissue:
    density: NiftiRef
    T1: float | NiftiRef | NiftiMapping
    T2: float | NiftiRef | NiftiMapping
    T2dash: float | NiftiRef | NiftiMapping
    ADC: float | NiftiRef | NiftiMapping
    dB0: float | NiftiRef | NiftiMapping
    B1_tx: list[float | NiftiRef | NiftiMapping]
    B1_rx: list[float | NiftiRef | NiftiMapping]

    @classmethod
    def default(cls, density: NiftiRef):
        return cls.from_dict({"density": density})

    @classmethod
    def from_dict(cls, config: dict[str, Any]):
        warn_unknown_fields(
            "a tissue",
            config,
            {"density", "T1", "T2", "T2'", "ADC", "dB0", "B1+", "B1-"},
        )

        def parse_prop(prop):
            if isinstance(prop, (float, int)):
                return float(prop)
            elif isinstance(prop, str):
                return NiftiRef.parse(prop)
            else:
                return NiftiMapping.parse(prop)

        return cls(
            density=NiftiRef.parse(config["density"]),
            T1=parse_prop(config.get("T1", float("inf"))),
            T2=parse_prop(config.get("T2", float("inf"))),
            T2dash=parse_prop(config.get("T2'", float("inf"))),
            ADC=parse_prop(config.get("ADC", 0.0)),
            dB0=parse_prop(config.get("dB0", 0.0)),
            B1_tx=[parse_prop(ch) for ch in config.get("B1+", [1.0])],
            B1_rx=[parse_prop(ch) for ch in config.get("B1-", [1.0])],
        )

    def to_dict(self) -> dict:
        def serialize_prop(prop):
            if isinstance(prop, (float, int)):
                return prop
            elif isinstance(prop, NiftiRef):
                return prop.to_str()
            elif isinstance(prop, NiftiMapping):
                return prop.to_dict()
            else:
                raise ValueError("Unsupported property type", type(prop))

        # Omit writing defaults (impossible for infinity)
        def is_default(prop, default):
            return isinstance(prop, (float, int)) and prop == default

        config: dict[str, Any] = {"density": self.density.to_str()}
        for key, prop, default in (
            ("T1", self.T1, float("inf")),
            ("T2", self.T2, float("inf")),
            ("T2'", self.T2dash, float("inf")),
            ("ADC", self.ADC, 0.0),
            ("dB0", self.dB0, 0.0),
        ):
            if not is_default(prop, default):
                config[key] = serialize_prop(prop)

        for key, channels in (("B1+", self.B1_tx), ("B1-", self.B1_rx)):
            if not (len(channels) == 1 and is_default(channels[0], 1.0)):
                config[key] = [serialize_prop(ch) for ch in channels]

        return config


@dataclass
class ResliceTo:
    affine: list[list[float]]
    resolution: list[int]

    @classmethod
    def from_dict(cls, config: dict[str, Any]):
        warn_unknown_fields("reslice_to", config, {"affine", "resolution"})
        return cls(
            affine=[[float(v) for v in row] for row in config["affine"]],
            resolution=[int(v) for v in config["resolution"]],
        )

    def to_dict(self) -> dict[str, Any]:
        return {"affine": self.affine, "resolution": self.resolution}


@dataclass
class Patient:
    """How the subject lies in the scanner (../JSON.md -> ``patient``).

    Phantom data is always stored subject-aligned in RAS+; this is what relates
    it to the scanner coordinate system a sequence is written in.
    """

    position: str  # one of PATIENT_POSITIONS

    @classmethod
    def from_dict(cls, config: dict[str, Any]):
        warn_unknown_fields("patient", config, {"position"})
        position = config["position"]
        if position not in PATIENT_POSITIONS:
            raise ValueError(
                f"Unknown patient position {position!r}, "
                f"expected one of {sorted(PATIENT_POSITIONS)}"
            )
        return cls(position=position)

    def to_dict(self) -> dict[str, Any]:
        return {"position": self.position}

    def to_scanner_matrix(self) -> list[list[float]]:
        """The 3x3 phantom-RAS+ -> scanner rotation of this position."""
        return patient_to_scanner(self.position)


@dataclass
class BiftiPhantom:
    # schema has to be any URL to a file named "bifti-phantom-v1".
    # Default to the file hosted on the offical GitHub repository.
    DEFAULT_SCHEMA = (
        "https://raw.githubusercontent.com/mrx-org/bifti-phantoms/"
        "refs/heads/main/bifti-phantom-v1.schema.json"
    )

    units: PhantomUnits
    system: PhantomSystem
    tissues: dict[str, BiftiTissue]
    reslice_to: ResliceTo | None = None
    schema: str = DEFAULT_SCHEMA
    # Omitted means FFS: an unpositioned phantom is never transformed.
    patient: Patient | None = None

    @classmethod
    def default(cls, gyro=42.5764, B0=3.0):
        return cls(PhantomUnits.default(), PhantomSystem(gyro, B0), {})

    def to_scanner_matrix(self) -> list[list[float]]:
        """The 3x3 phantom-RAS+ -> scanner rotation of this phantom.

        The identity if no ``patient`` is given (../NIFTI.md#patient-position).
        """
        return patient_to_scanner(self.patient.position if self.patient else None)

    @classmethod
    def load(cls, path: Path | str):
        import json

        with open(path, "r") as f:
            config = json.load(f)
        return cls.from_dict(config)

    def save(self, path: Path | str):
        import json
        import os

        path = Path(path)

        os.makedirs(path.parent, exist_ok=True)
        with open(path, "w") as f:
            json.dump(self.to_dict(), f, indent=2, allow_nan=False)

    @classmethod
    def from_dict(cls, config: dict):
        import re

        schema = config["$schema"]
        assert re.search(
            r"(nifti|bifti)-phantom-v1(\.[^/]*)?$", schema
        ), f"Unsupported $schema: {schema!r}"

        warn_unknown_fields(
            "the phantom",
            config,
            {"$schema", "units", "system", "patient", "reslice_to", "tissues"},
        )

        units = PhantomUnits.from_dict(config["units"])
        system = PhantomSystem.from_dict(config["system"])
        if "reslice_to" in config:
            reslice_to = ResliceTo.from_dict(config["reslice_to"])
        else:
            reslice_to = None
        if "patient" in config:
            patient = Patient.from_dict(config["patient"])
        else:
            patient = None
        tissues = {
            name: BiftiTissue.from_dict(tissue)
            for name, tissue in config["tissues"].items()
        }

        return cls(units, system, tissues, reslice_to, schema, patient)

    def to_dict(self) -> dict:
        config: dict[str, Any] = {
            "$schema": self.schema,
            "units": self.units.to_dict(),
            "system": self.system.to_dict(),
        }
        if self.patient is not None:
            config["patient"] = self.patient.to_dict()
        if self.reslice_to is not None:
            config["reslice_to"] = self.reslice_to.to_dict()
        config["tissues"] = {
            name: tissue.to_dict() for name, tissue in self.tissues.items()
        }
        return config

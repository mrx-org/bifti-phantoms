"""Simulate a Pulseq sequence on a NIfTI phantom with MR-zero.

Loads a phantom JSON via the standalone reference loader (``nifti_loader.py``),
which already honours the spec's ``reslice_to`` field — sidestepping MR-zero's
current lack of in-package ``reslice_to`` support (see PR
https://github.com/MRsources/MRzero-Core/pull/172).

Pipeline:

1. ``load_phantom`` → ``dict[str, NumpyTissue]`` on the target grid.
2. Each tissue wrapped as ``mr0.VoxelGridPhantom``.
3. ``TissueDict.combine().build()`` → partial-volume ``SimData``.
4. ``mr0.Sequence.import_file`` reads the ``.seq``.
5. ``compute_graph`` → ``execute_graph`` → ``reco_adjoint``.

Usage::

    python mrzero_sim.py PHANTOM.json SEQ.seq
    python mrzero_sim.py brainweb/subj04-2D.json demo/data/tse.seq \\
        --fov 0.256 0.256 1 --res 128 128 1

Dependencies: ``MRzeroCore``, ``torch``, ``numpy``, ``nibabel``, ``scipy``,
``matplotlib`` (plus the sibling ``nifti_loader.py`` / ``nifti_phantom.py``).
"""

from __future__ import annotations

import argparse
from pathlib import Path

import numpy as np
import torch
import matplotlib.pyplot as plt
import MRzeroCore as mr0

from nifti_loader import load_phantom, NumpyTissue


def to_voxel_grid(t: NumpyTissue) -> mr0.VoxelGridPhantom:
    """Wrap one (already-resliced) NumpyTissue as an ``mr0.VoxelGridPhantom``.

    The physical extent comes from the tissue's 3×4 affine: ``|shape · R|`` in
    mm, converted to meters. Matches the convention in
    ``MRzeroCore/phantom/tissue_dict.py:load_tissue`` so the same JSON yields
    the same FOV either way.
    """
    affine = np.asarray(t.affine, dtype=np.float64)
    shape = np.asarray(t.density.shape, dtype=np.float64)
    size_m = np.abs(shape @ affine[:, :3]) / 1000.0

    return mr0.VoxelGridPhantom(
        PD=torch.as_tensor(t.density, dtype=torch.float32),
        T1=torch.as_tensor(t.T1, dtype=torch.float32),
        T2=torch.as_tensor(t.T2, dtype=torch.float32),
        T2dash=torch.as_tensor(t.T2dash, dtype=torch.float32),
        D=torch.as_tensor(t.ADC, dtype=torch.float32),
        B0=torch.as_tensor(t.dB0, dtype=torch.float32),
        B1=torch.as_tensor(t.B1_tx, dtype=torch.complex64),
        coil_sens=torch.as_tensor(t.B1_rx, dtype=torch.complex64),
        size=torch.as_tensor(size_m, dtype=torch.float32),
    )


def _concat_simdata(per_tissue: list[mr0.SimData]) -> mr0.SimData:
    """Concatenate per-tissue SimData into one multi-tissue SimData.

    Same body as ``mr0.TissueDict.build``'s tail end, but without the
    ``torch.stack(tissue_masks)`` line — that line tries to stack ``dict``
    objects and currently raises on multi-tissue builds.
    """
    return mr0.SimData(
        PD=torch.cat([o.PD for o in per_tissue]),
        T1=torch.cat([o.T1 for o in per_tissue]),
        T2=torch.cat([o.T2 for o in per_tissue]),
        T2dash=torch.cat([o.T2dash for o in per_tissue]),
        D=torch.cat([o.D for o in per_tissue]),
        B0=torch.cat([o.B0 for o in per_tissue]),
        B1=torch.cat([o.B1 for o in per_tissue], 1),
        coil_sens=torch.cat([o.coil_sens for o in per_tissue], 1),
        voxel_pos=torch.cat([o.voxel_pos for o in per_tissue], 0),
        size=per_tissue[0].size,
        nyquist=per_tissue[0].nyquist,
        dephasing_func=per_tissue[0].dephasing_func,
    )


def build_simdata(phantom_json: Path, combine: bool = False) -> mr0.SimData:
    """Load a NIfTI phantom JSON and build a SimData.

    ``combine=False`` (default) keeps one spin per (tissue, voxel) carrying
    that tissue's own T1/T2/... — matches the converter in ``nifti_to_koma.py``
    so MR-zero and Koma model partial-volume voxels identically. With
    ``combine=True`` tissue parameters are averaged into one spin per voxel;
    cheaper, but signal at long TE drifts because exp(-t/T2) is convex in T2.
    """
    tissues = load_phantom(phantom_json)
    grids = mr0.TissueDict({name: to_voxel_grid(t) for name, t in tissues.items()})
    if combine:
        return grids.combine().build()
    return _concat_simdata([g.build() for g in grids.values()])


def main() -> None:
    p = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    p.add_argument("phantom", type=Path, help="path to the phantom JSON")
    p.add_argument("seq", type=Path, help="path to the .seq file")
    p.add_argument("--fov", nargs=3, type=float, metavar=("FX", "FY", "FZ"),
                   default=None,
                   help="recon FOV in meters (default: auto from k-space)")
    p.add_argument("--res", nargs=3, type=int, metavar=("NX", "NY", "NZ"),
                   default=None,
                   help="recon resolution in voxels (default: auto from k-space)")
    p.add_argument("--vmax", type=float, default=None,
                   help="upper clip for the magnitude colormap")
    p.add_argument("--combine", action="store_true",
                   help="partial-volume-average tissues into one spin per voxel "
                        "(default: one spin per (tissue, voxel), matches "
                        "nifti_to_koma.py)")
    p.add_argument("--out", type=Path, default=None,
                   help="if given, save the figure to this path")
    args = p.parse_args()

    print(f"loading phantom: {args.phantom}")
    data = build_simdata(args.phantom, combine=args.combine)
    print(f"  spins={data.PD.numel()}, FOV={data.size.tolist()} m")

    print(f"loading seq:     {args.seq}")
    seq = mr0.Sequence.import_file(str(args.seq))
    kspace = seq.get_kspace()

    print("simulating ...")
    graph = mr0.compute_graph(seq, data)
    signal = mr0.execute_graph(graph, seq, data)

    fov = tuple(args.fov) if args.fov else None
    res = tuple(args.res) if args.res else None
    reco = mr0.reco_adjoint(signal, kspace, FOV=fov, resolution=res)

    fig = plt.figure(figsize=(7, 7))
    mr0.util.imshow(reco.abs(), cmap="gray", vmin=0, vmax=args.vmax)
    plt.title(f"{args.phantom.stem}  /  {args.seq.stem}")
    plt.colorbar(fraction=0.046, pad=0.04)
    plt.tight_layout()

    if args.out is not None:
        fig.savefig(args.out, dpi=110)
        print(f"saved {args.out}")
    plt.show()


if __name__ == "__main__":
    main()

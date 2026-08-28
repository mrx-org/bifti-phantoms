"""Array backend for resampling: torch when it is installed, NumPy otherwise.

The separable resampling path (see :mod:`bifti.resample`) is three matrix contractions, so
it maps straight onto whichever array library is available. Torch is used whenever it can be
imported - *not* only when CUDA is present, since torch is faster than NumPy on CPU too and
falls back to the same code path either way.
"""

from __future__ import annotations

import os
import warnings
from functools import lru_cache

import numpy as np


class Backend:
    """The handful of array operations resampling needs."""

    name = "numpy"

    def asarray(self, x):
        return np.asarray(x, dtype=np.float64)

    def to_numpy(self, x) -> np.ndarray:
        return np.asarray(x, dtype=np.float64)

    def tensordot(self, a, b, axes):
        return np.tensordot(a, b, axes=axes)

    def moveaxis(self, x, source, destination):
        return np.moveaxis(x, source, destination)

    def transpose(self, x, axes):
        return np.transpose(x, axes)


class TorchBackend(Backend):
    """Torch-backed contractions, on CUDA when available.

    Uses float64 on CPU so results match the NumPy backend exactly, and float32 on CUDA
    where float64 throughput is heavily penalised. That makes the GPU path accurate to
    roughly 1e-6 relative rather than to machine precision - far below the accuracy of the
    phantom data itself.
    """

    name = "torch"

    def __init__(self, torch):
        self._torch = torch
        self.device = torch.device("cuda" if torch.cuda.is_available() else "cpu")
        self.dtype = torch.float32 if self.device.type == "cuda" else torch.float64

    def asarray(self, x):
        if self._torch.is_tensor(x):
            return x.to(device=self.device, dtype=self.dtype)
        return self._torch.as_tensor(
            np.ascontiguousarray(x, dtype=np.float64),
            dtype=self.dtype,
            device=self.device,
        )

    def to_numpy(self, x) -> np.ndarray:
        if self._torch.is_tensor(x):
            return x.detach().to("cpu", self._torch.float64).numpy()
        return np.asarray(x, dtype=np.float64)

    def tensordot(self, a, b, axes):
        return self._torch.tensordot(a, b, dims=axes)

    def moveaxis(self, x, source, destination):
        return self._torch.movedim(x, source, destination)

    def transpose(self, x, axes):
        return self._torch.permute(x, tuple(int(a) for a in axes))


def _load_torch_backend() -> Backend | None:
    try:
        import torch
    except ImportError:
        return None
    try:
        return TorchBackend(torch)
    except Exception as exc:  # pragma: no cover - torch present but unusable
        warnings.warn(f"torch is installed but unusable ({exc}); falling back to NumPy")
        return None


@lru_cache(maxsize=None)
def get_backend(name: str | None = None) -> Backend:
    """The array backend to resample with.

    ``name`` (or ``$BIFTI_RESAMPLE_BACKEND``) may be ``"auto"`` (the default), ``"numpy"``
    or ``"torch"``. ``"torch"`` raises if torch is missing; ``"auto"`` quietly falls back.
    """
    name = (name or os.environ.get("BIFTI_RESAMPLE_BACKEND", "auto")).strip().lower()
    if name == "numpy":
        return Backend()
    if name == "torch":
        backend = _load_torch_backend()
        if backend is None:
            raise ImportError("BIFTI_RESAMPLE_BACKEND=torch but torch is not installed")
        return backend
    if name != "auto":
        raise ValueError(
            f"invalid resampling backend {name!r} (use auto, numpy or torch)"
        )
    return _load_torch_backend() or Backend()

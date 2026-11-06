"""Optional NumPy access; importing cq_jt does not require NumPy."""
from .mesh import PackedArray


def as_numpy(array: PackedArray):
    """Return a read-only view over the explicit PackedArray copy."""
    import numpy as np
    dtype = {"f64": "<f8", "u32": "<u4", "i32": "<i4"}[array.dtype]
    return np.frombuffer(array.data, dtype=dtype).reshape(array.shape)

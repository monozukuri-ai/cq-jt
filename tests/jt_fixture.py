"""Generate framing only; the payload deliberately is not a compressed JT stream."""
import struct
import lzma
import zlib


def fixture(version="9.5", endian="<", *, tail=False, extra=False, unknown=False):
    width = 8 if version.startswith("10.") else 4
    offset_format = "Q" if width == 8 else "i"
    header_size = 101 + width + (16 if extra else 0)
    toc_length = 4 + 2 * (24 + width)
    ids = (bytes(range(16)), bytes(range(16, 32)))
    payloads = (struct.pack(endian + "IiB", 2, 6, 2) + b"\xff" * 5, b"opaque")
    kinds = (1, 250 if unknown else 7)
    lengths = tuple(24 + len(p) for p in payloads)
    first = header_size if tail else header_size + toc_length
    offsets = (first, first + lengths[0])
    toc_offset = first + sum(lengths) if tail else header_size
    header = f"Version {version} synthetic framing".encode().ljust(75, b" ") + b" \n\r\n "
    header += struct.pack(endian + "Bi" + offset_format, int(endian == ">"), int(extra), toc_offset) + ids[0]
    if extra:
        header += b"\xab" * 16
    toc = struct.pack(endian + "i", 2)
    segments = b""
    for guid, offset, length, kind, payload in zip(ids, offsets, lengths, kinds, payloads):
        toc += guid + struct.pack(endian + offset_format + "II", offset, length, kind << 24 | 0x123)
        segments += guid + struct.pack(endian + "ii", kind, length) + payload
    return header + (segments + toc if tail else toc + segments)


def logical_element(payload=b"\0\xffopaque", *, version="9.5", endian="<", type_id=b"\x31" * 16, object_id=-7, base_type=255):
    header = type_id + bytes([base_type])
    if not version.startswith("8."):
        header += struct.pack(endian + "i", object_id)
    return struct.pack(endian + "i", len(header) + len(payload)) + header + payload


def end_marker(endian="<"):
    return struct.pack(endian + "i", 16) + b"\xff" * 16


def lsg_payload(version="9.5", endian="<"):
    e = logical_element(version=version, endian=endian)
    marker = end_marker(endian)
    if tuple(map(int, version.split("."))) >= (10, 5):
        return e + e + marker + b"\x01\0\0\0\0\0"
    return e + marker + e + marker + b"\x01\0\0\0\0\0"


def container_for_payloads(entries=None, *, version="9.5", endian="<"):
    """entries = [(segment type, decoded bytes, codec)]; first must be the LSG."""
    entries = entries or [(1, lsg_payload(version, endian), "zlib")]
    width = 8 if version.startswith("10.") else 4
    fmt = "Q" if width == 8 else "i"
    header_size = 101 + width
    position = header_size + 4 + len(entries) * (24 + width)
    toc = struct.pack(endian + "i", len(entries))
    segments = b""
    root = bytes(range(16))
    for index, (kind, decoded, codec) in enumerate(entries):
        guid = bytes((i + index) % 256 for i in range(16))
        if codec == "zlib":
            stream = zlib.compress(decoded)
            payload = struct.pack(endian + "IiB", 2, len(stream) + 1, 2) + stream
        elif codec == "xz":
            stream = lzma.compress(decoded, format=lzma.FORMAT_XZ, filters=[{"id": lzma.FILTER_LZMA2, "dict_size": 4096}])
            payload = struct.pack(endian + "IiB", 3, len(stream) + 1, 3) + stream
        elif codec == "none":
            payload = struct.pack(endian + "IiB", 0, len(decoded) + 1, 1) + decoded
        elif codec == "raw":
            payload = decoded
        else:
            raise ValueError(codec)
        length = len(payload) + 24
        toc += guid + struct.pack(endian + fmt + "II", position, length, kind << 24)
        segments += guid + struct.pack(endian + "ii", kind, length) + payload
        position += length
    header = f"Version {version} synthetic payload".encode().ljust(75, b" ") + b" \n\r\n "
    header += struct.pack(endian + "Bi" + fmt, int(endian == ">"), 0, header_size) + root
    return header + toc + segments

"""Runtime helpers for generated Mycelium protocol bindings."""

from __future__ import annotations

from dataclasses import dataclass
from typing import Any, Dict, List, Sequence, Tuple, Type, TypeVar

import struct

T = TypeVar("T")


class FieldType:
    """Base class for all generated field encoders/decoders."""

    size: int

    def encode(self, value: Any, buffer: bytearray, offset: int) -> int:  # pragma: no cover - interface
        raise NotImplementedError

    def decode(self, view: memoryview, offset: int) -> Tuple[Any, int]:  # pragma: no cover - interface
        raise NotImplementedError


class PrimitiveField(FieldType):
    def __init__(self, fmt: str):
        self.fmt = fmt
        self.size = struct.calcsize(fmt)

    def encode(self, value: Any, buffer: bytearray, offset: int) -> int:
        struct.pack_into(self.fmt, buffer, offset, value)
        return offset + self.size

    def decode(self, view: memoryview, offset: int) -> Tuple[Any, int]:
        value = struct.unpack_from(self.fmt, view, offset)[0]
        return value, offset + self.size


class FixedBytesField(FieldType):
    def __init__(self, length: int):
        self.size = length

    def encode(self, value: Any, buffer: bytearray, offset: int) -> int:
        data = bytes(value)
        if len(data) != self.size:
            raise ValueError(f"expected {self.size} bytes, got {len(data)}")
        buffer[offset:offset + self.size] = data
        return offset + self.size

    def decode(self, view: memoryview, offset: int) -> Tuple[bytes, int]:
        data = bytes(view[offset:offset + self.size])
        return data, offset + self.size


class FixedStrField(FieldType):
    def __init__(self, capacity: int):
        self.capacity = capacity
        self.size = 2 + 6 + capacity

    def encode(self, value: str, buffer: bytearray, offset: int) -> int:
        encoded = value.encode("utf-8")
        if len(encoded) > self.capacity:
            raise ValueError(f"string exceeds capacity {self.capacity}")

        struct.pack_into("<H", buffer, offset, len(encoded))
        offset += 2
        buffer[offset:offset + 6] = b"\x00" * 6
        offset += 6

        start = offset
        buffer[start:start + self.capacity] = b"\x00" * self.capacity
        buffer[start:start + len(encoded)] = encoded
        return offset + self.capacity

    def decode(self, view: memoryview, offset: int) -> Tuple[str, int]:
        length = struct.unpack_from("<H", view, offset)[0]
        offset += 2
        offset += 6
        data = bytes(view[offset:offset + self.capacity])
        offset += self.capacity
        return data[:length].decode("utf-8"), offset


class FixedVecBytesField(FieldType):
    def __init__(self, element_size: int, capacity: int):
        self.element_size = element_size
        self.capacity = capacity
        self.size = 2 + 6 + element_size * capacity

    def encode(self, value: Sequence[Any], buffer: bytearray, offset: int) -> int:
        count = len(value)
        if count > self.capacity:
            raise ValueError(
                f"vector exceeds capacity {self.capacity}: attempted {count}"
            )

        struct.pack_into("<H", buffer, offset, count)
        offset += 2
        buffer[offset:offset + 6] = b"\x00" * 6
        offset += 6

        for idx in range(self.capacity):
            start = offset + idx * self.element_size
            buffer[start:start + self.element_size] = b"\x00" * self.element_size
            if idx >= count:
                continue

            elem = value[idx]
            if self.element_size == 1:
                buffer[start] = int(elem) & 0xFF
            else:
                chunk = bytes(elem)
                if len(chunk) != self.element_size:
                    raise ValueError(
                        f"element {idx} must be {self.element_size} bytes, got {len(chunk)}"
                    )
                buffer[start:start + self.element_size] = chunk

        return offset + self.element_size * self.capacity

    def decode(self, view: memoryview, offset: int) -> Tuple[List[Any], int]:
        count = struct.unpack_from("<H", view, offset)[0]
        offset += 2
        offset += 6
        values: List[Any] = []

        for idx in range(self.capacity):
            start = offset + idx * self.element_size
            chunk = bytes(view[start:start + self.element_size])
            if idx < count:
                if self.element_size == 1:
                    values.append(chunk[0])
                else:
                    values.append(chunk)

        offset += self.element_size * self.capacity
        return values, offset


class _PrimitiveNamespace:
    def __init__(self) -> None:
        self.U8 = PrimitiveField("<B")
        self.U16 = PrimitiveField("<H")
        self.U32 = PrimitiveField("<I")
        self.U64 = PrimitiveField("<Q")
        self.I32 = PrimitiveField("<i")
        self.F64 = PrimitiveField("<d")


Primitive = _PrimitiveNamespace()


def FixedBytes(length: int) -> FieldType:
    return FixedBytesField(length)


def FixedStr(capacity: int) -> FieldType:
    return FixedStrField(capacity)


def FixedVecBytes(element_size: int, capacity: int) -> FieldType:
    return FixedVecBytesField(element_size, capacity)


@dataclass(frozen=True)
class FieldDef:
    name: str
    field_type: FieldType


@dataclass
class MessageDef:
    name: str
    type_id: int
    topic: str
    fields: Tuple[FieldDef, ...]
    dataclass: Type[Any]

    def __post_init__(self) -> None:
        if not isinstance(self.fields, tuple):
            self.fields = tuple(self.fields)
        self.size = sum(field.field_type.size for field in self.fields)

    def encode(self, obj: Any) -> bytes:
        buffer = bytearray(self.size)
        offset = 0
        for field in self.fields:
            value = getattr(obj, field.name)
            offset = field.field_type.encode(value, buffer, offset)
        return bytes(buffer)

    def decode(self, cls: Type[T], payload: bytes) -> T:
        if len(payload) != self.size:
            raise ValueError(
                f"payload size mismatch: expected {self.size}, got {len(payload)}"
            )

        view = memoryview(payload)
        offset = 0
        kwargs: Dict[str, Any] = {}
        for field in self.fields:
            value, offset = field.field_type.decode(view, offset)
            kwargs[field.name] = value
        return cls(**kwargs)


_MESSAGES_BY_ID: Dict[int, MessageDef] = {}


def register_message(definition: MessageDef) -> None:
    if definition.type_id in _MESSAGES_BY_ID:
        raise ValueError(f"duplicate message type {definition.type_id}")
    _MESSAGES_BY_ID[definition.type_id] = definition


def get_message_definition(type_id: int) -> MessageDef:
    return _MESSAGES_BY_ID[type_id]


__all__ = [
    "FieldDef",
    "FixedBytes",
    "FixedStr",
    "FixedVecBytes",
    "MessageDef",
    "Primitive",
    "register_message",
    "get_message_definition",
]

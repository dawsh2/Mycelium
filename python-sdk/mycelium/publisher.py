from __future__ import annotations

from typing import Callable, Type

from .codec import encode_frame


class Publisher:
    """Publishes typed messages over an underlying transport."""

    def __init__(
        self,
        message_cls: Type,
        sender: Callable[[bytes], None],
    ) -> None:
        self._message_cls = message_cls
        self._sender = sender
        self._definition = message_cls.__definition__

    @property
    def message_cls(self) -> Type:
        return self._message_cls

    def publish(self, message: object) -> None:
        if not isinstance(message, self._message_cls):
            raise TypeError(
                f"publisher for {self._message_cls.__name__} received {type(message).__name__}"
            )
        payload = message.to_bytes()
        frame = encode_frame(self._definition.type_id, payload)
        self._sender(frame)


__all__ = ["Publisher"]

[@@@ocaml.warning "-33"]

open Messages

type t
type subscription

external create_raw : bytes -> t = "caml_mycelium_native_create"
external close : t -> unit = "caml_mycelium_native_close"
external publish_raw : t -> int -> bytes -> unit = "caml_mycelium_native_publish_raw"
external subscribe_raw : t -> int -> (bytes -> unit) -> subscription
  = "caml_mycelium_native_subscribe_raw"
external unsubscribe : subscription -> unit = "caml_mycelium_native_unsubscribe"

let create ?(schema_digest = Messages.schema_digest) () =
  create_raw schema_digest

let publish runtime ~type_id encode msg =
  publish_raw runtime type_id (encode msg)

let subscribe t ~type_id ~decode ~callback =
  subscribe_raw t type_id (fun payload -> callback (decode payload))

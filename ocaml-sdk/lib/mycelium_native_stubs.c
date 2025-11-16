#include <caml/alloc.h>
#include <caml/callback.h>
#include <caml/custom.h>
#include <caml/fail.h>
#include <caml/memory.h>
#include <caml/mlvalues.h>
#include <caml/printexc.h>
#include <caml/threads.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include "mycelium_ffi.h"

typedef struct {
  MyceliumRuntime *ptr;
} runtime_handle;

typedef struct {
  runtime_handle *runtime;
  uint64_t subscription_id;
  value closure;
  int active;
} subscription_handle;

static void runtime_finalize(value v);
static void subscription_finalize(value v);

static struct custom_operations runtime_ops = {
  .identifier = "mycelium.native.runtime",
  .finalize = runtime_finalize,
  .compare = custom_compare_default,
  .hash = custom_hash_default,
  .serialize = custom_serialize_default,
  .deserialize = custom_deserialize_default,
  .compare_ext = custom_compare_ext_default,
};

static struct custom_operations subscription_ops = {
  .identifier = "mycelium.native.subscription",
  .finalize = subscription_finalize,
  .compare = custom_compare_default,
  .hash = custom_hash_default,
  .serialize = custom_serialize_default,
  .deserialize = custom_deserialize_default,
  .compare_ext = custom_compare_ext_default,
};

static runtime_handle *runtime_val(value v) {
  return (runtime_handle *)Data_custom_val(v);
}

static subscription_handle *subscription_val(value v) {
  return *((subscription_handle **)Data_custom_val(v));
}

static void runtime_finalize(value v) {
  runtime_handle *rt = runtime_val(v);
  if (rt->ptr != NULL) {
    mycelium_runtime_destroy(rt->ptr);
    rt->ptr = NULL;
  }
}

static void release_subscription(subscription_handle *sub) {
  if (sub == NULL) {
    return;
  }
  if (sub->active) {
    sub->active = 0;
    if (sub->runtime->ptr != NULL) {
      mycelium_unsubscribe(sub->runtime->ptr, sub->subscription_id);
    }
    caml_remove_generational_global_root(&sub->closure);
  }
  free(sub);
}

static void subscription_finalize(value v) {
  subscription_handle *sub = subscription_val(v);
  release_subscription(sub);
}

static void raise_runtime_error(const char *where, int32_t code) {
  char message[128];
  snprintf(message, sizeof(message), "%s failed (%d)", where, code);
  caml_failwith(message);
}

static void dispatch_callback(
    uint16_t type_id,
    const uint8_t *payload_ptr,
    size_t payload_len,
    void *user_data) {
  subscription_handle *sub = (subscription_handle *)user_data;
  if (!sub->active) {
    return;
  }
  caml_acquire_runtime_system();
  CAMLparam0();
  CAMLlocal1(payload);
  payload = caml_alloc_initialized_string(payload_len, (const char *)payload_ptr);
  value result = caml_callback_exn(sub->closure, payload);
  if (Is_exception_result(result)) {
    value exn = Extract_exception(result);
    char *msg = caml_format_exception(exn);
    fprintf(stderr, "OCaml callback raised: %s\n", msg);
    caml_stat_free(msg);
  }
  CAMLdrop;
  caml_release_runtime_system();
  (void)type_id;
}

CAMLprim value caml_mycelium_native_create(value digest_bytes) {
  CAMLparam1(digest_bytes);
  size_t digest_len = caml_string_length(digest_bytes);
  runtime_handle *rt;
  value block = caml_alloc_custom_mem(&runtime_ops, sizeof(runtime_handle), 0);
  rt = runtime_val(block);
  rt->ptr = mycelium_runtime_create();
  if (rt->ptr == NULL) {
    caml_failwith("mycelium_runtime_create failed");
  }
  if (digest_len > 0) {
    int32_t status = mycelium_verify_schema_digest(
        rt->ptr,
        (const uint8_t *)Bytes_val(digest_bytes),
        digest_len);
    if (status != SUCCESS) {
      mycelium_runtime_destroy(rt->ptr);
      rt->ptr = NULL;
      raise_runtime_error("mycelium_verify_schema_digest", status);
    }
  }
  CAMLreturn(block);
}

CAMLprim value caml_mycelium_native_close(value runtime_v) {
  CAMLparam1(runtime_v);
  runtime_handle *rt = runtime_val(runtime_v);
  if (rt->ptr != NULL) {
    mycelium_runtime_destroy(rt->ptr);
    rt->ptr = NULL;
  }
  CAMLreturn(Val_unit);
}

CAMLprim value caml_mycelium_native_publish_raw(value runtime_v, value type_id_v, value payload_v) {
  CAMLparam3(runtime_v, type_id_v, payload_v);
  runtime_handle *rt = runtime_val(runtime_v);
  if (rt->ptr == NULL) {
    caml_failwith("runtime closed");
  }
  size_t len = caml_string_length(payload_v);
  const uint8_t *data = (const uint8_t *)Bytes_val(payload_v);
  int32_t status = mycelium_publish(rt->ptr, Int_val(type_id_v), data, len);
  if (status != SUCCESS) {
    raise_runtime_error("mycelium_publish", status);
  }
  CAMLreturn(Val_unit);
}

CAMLprim value caml_mycelium_native_subscribe_raw(value runtime_v, value type_id_v, value callback_v) {
  CAMLparam3(runtime_v, type_id_v, callback_v);
  CAMLlocal1(block);
  runtime_handle *rt = runtime_val(runtime_v);
  if (rt->ptr == NULL) {
    caml_failwith("runtime closed");
  }
  subscription_handle *sub = malloc(sizeof(subscription_handle));
  if (sub == NULL) {
    caml_failwith("malloc failed");
  }
  sub->runtime = rt;
  sub->subscription_id = 0;
  sub->closure = callback_v;
  sub->active = 0;
  caml_register_generational_global_root(&sub->closure);
  uint64_t subscription_id = 0;
  int32_t status = mycelium_subscribe(
      rt->ptr,
      Int_val(type_id_v),
      dispatch_callback,
      sub,
      &subscription_id);
  if (status != SUCCESS) {
    caml_remove_generational_global_root(&sub->closure);
    free(sub);
    raise_runtime_error("mycelium_subscribe", status);
  }
  sub->subscription_id = subscription_id;
  sub->active = 1;
  block = caml_alloc_custom_mem(&subscription_ops, sizeof(subscription_handle *), 0);
  *((subscription_handle **)Data_custom_val(block)) = sub;
  CAMLreturn(block);
}

CAMLprim value caml_mycelium_native_unsubscribe(value subscription_v) {
  CAMLparam1(subscription_v);
  subscription_handle *sub = subscription_val(subscription_v);
  release_subscription(sub);
  *((subscription_handle **)Data_custom_val(subscription_v)) = NULL;
  CAMLreturn(Val_unit);
}

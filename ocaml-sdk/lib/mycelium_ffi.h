#ifndef MYCELIUM_FFI_H
#define MYCELIUM_FFI_H

#include <stddef.h>
#include <stdint.h>

typedef struct MyceliumRuntime MyceliumRuntime;

typedef void (*MyceliumMessageCallback)(
    uint16_t type_id,
    const uint8_t *payload_ptr,
    size_t payload_len,
    void *user_data);

int32_t mycelium_publish(
    MyceliumRuntime *handle,
    uint16_t type_id,
    const uint8_t *payload_ptr,
    size_t payload_len);

int32_t mycelium_subscribe(
    MyceliumRuntime *handle,
    uint16_t type_id,
    MyceliumMessageCallback callback,
    void *user_data,
    uint64_t *out_subscription_id);

int32_t mycelium_unsubscribe(MyceliumRuntime *handle, uint64_t subscription_id);

MyceliumRuntime *mycelium_runtime_create(void);
void mycelium_runtime_destroy(MyceliumRuntime *handle);

int32_t mycelium_verify_schema_digest(
    MyceliumRuntime *handle,
    const uint8_t *digest_ptr,
    size_t digest_len);

extern const int32_t SUCCESS;
extern const int32_t ERR_NULL;
extern const int32_t ERR_DIGEST;
extern const int32_t ERR_RUNTIME;
extern const int32_t ERR_SUBSCRIPTION;

#endif // MYCELIUM_FFI_H

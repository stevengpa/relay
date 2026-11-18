# How the relay works

The relay is a Cargo workspace with three crates. `protogen` compiles `proto/relay.proto`. `server` is the HTTP gateway and the gRPC service. `client` is a worker that opens one long-lived stream and forwards every request to `TARGET_URL`.

```
HTTP caller
    |
    |  any method, path, headers, body
    v
server :8080                         axum
    |  1. allocate id, store a oneshot
    |  2. broadcast HttpRequestMsg
    v
server :50051                        tonic, one bidi stream per worker
    |----------------|----------------|
    v                v                v
 client A         client B         client C          reqwest
    |                |                |
    v                v                v
 TARGET_URL       TARGET_URL       TARGET_URL
    |
    |  HttpResponseMsg (same id)
    v
server
    the first response for that id completes the oneshot
    later responses with the same id are dropped
    |
    v
HTTP caller receives that status, headers, and body
```

Workers do not subscribe to a subset of routes. Each one receives every request and, in this implementation, forwards every one.

## Layout

```
Cargo.toml                 workspace: protogen, server, client
proto/relay.proto
protogen/
  build.rs                 tonic-build
  src/lib.rs               tonic::include_proto!("relay")
server/src/
  main.rs                  bind HTTP_PORT and GRPC_PORT
  http.rs                  capture the HTTP call and broadcast it
  service.rs               RelayService::forward
  state.rs                 pending oneshots, client senders, next id
client/src/main.rs         connect, forward, reply
```

## Libraries

| Crate | Where | Role |
| --- | --- | --- |
| `tonic` 0.12 | protogen, server, client | gRPC server, client, and `Status` |
| `prost` 0.13 | protogen, server, client | protobuf messages |
| `tonic-build` 0.12 | protogen build script | generate Rust from `relay.proto` |
| `tokio` 1 (full) | server, client | runtime, `mpsc`, `oneshot`, tasks |
| `tokio-stream` 0.1 | server, client | `ReceiverStream` over an `mpsc` channel |
| `axum` 0.7 | server | HTTP gateway |
| `bytes` 1 | server | request body type used by axum |
| `reqwest` 0.12 | client | outbound HTTP to `TARGET_URL` |

`tonic-build` shells out to `protoc`. The package name in the proto file is `relay`, which is the argument to `tonic::include_proto!`.

## Protocol

`RelayService.Forward` is one bidirectional stream.

```protobuf
rpc Forward(stream HttpResponseMsg) returns (stream HttpRequestMsg);
```

The worker opens the stream. The server then sends `HttpRequestMsg` and the worker sends `HttpResponseMsg`. The direction looks reversed if you read it as a normal RPC: the worker is the one that initiates the call, and the payloads it sends are the HTTP responses.

`HttpRequestMsg` carries `id`, `method`, `path` (path plus query string), headers, and the full body. `HttpResponseMsg` carries the same `id`, an HTTP status, headers, and the full body. Headers are repeated `key` / `value` strings, not a map.

The whole body is buffered in memory. There is no chunked upload and no progressive multipart handling. gRPC's default message size is about 4 MB; this server does not raise `max_message_size`.

## Shared state

`AppState` is cloned into the axum router and the gRPC service. The three fields are `Arc`s:

- `pending` — `HashMap<u64, oneshot::Sender<Result<HttpResponseMsg, Status>>>`. One entry per HTTP call that is still waiting.
- `clients` — `Vec<mpsc::Sender<Result<HttpRequestMsg, Status>>>`. One sender per connected worker. The `Result` is there because tonic's server stream item is `Result<Message, Status>`. The gateway always sends `Ok`.
- `next_id` — `AtomicU64`, incremented with `Ordering::Relaxed`.

There is no round-robin cursor. Delivery is a broadcast.

## HTTP gateway

`server` binds `0.0.0.0` on `HTTP_PORT` (default `8080`) and routes every method through `http::handle`. The axum 0.7 catch-all is `/*rest`. A separate `fallback` covers `/`, which the wildcard does not match. The gRPC service binds `0.0.0.0` on `GRPC_PORT` (default `50051`).

For each call:

1. Lock `clients`, drop senders whose channel is closed, and clone the remaining list so the lock is not held during I/O.
2. If the list is empty, return `503` with `no gRPC clients connected`.
3. Allocate an id and insert a oneshot sender into `pending` before anything is broadcast, so a fast worker cannot reply before the waiter exists.
4. Build `HttpRequestMsg`. Header values that are not valid UTF-8 are skipped. The body is copied into a `Vec<u8>`.
5. `send` that message to every worker. A failed send is logged and skipped; it does not cancel the others. The channel capacity is 32. `send` waits when a worker's channel is full, so one slow worker stalls the broadcast for that request.
6. Await the oneshot. The first `HttpResponseMsg` is turned back into an axum response: status, headers, body. Any other outcome (the oneshot sender was dropped) returns `502` with `no client returned a response`.

The wait has no timeout. A call with live workers that never reply stays open.

## gRPC service

`Relay::forward` runs once per worker connection.

1. Create an `mpsc` channel of `Result<HttpRequestMsg, Status>` with capacity 32.
2. Push a clone of the sender onto `clients`. That is the broadcast registration.
3. Spawn a task that reads `HttpResponseMsg` from the worker. For each message it `remove`s `pending[id]`. Only the task that wins the remove still holds the oneshot sender, and it completes that sender. A second response for the same id finds nothing and is discarded.
4. Return the channel's receiver as the server-streaming side, wrapped in `ReceiverStream`.

When the worker stream ends, the task removes its sender with `same_channel`, so later broadcasts skip that worker. The HTTP path also drops senders that report `is_closed`.

## Worker

The client reads `TARGET_URL` (required) and `SERVER_ADDR` (default `http://127.0.0.1:50051`). It connects with `RelayServiceClient` and calls `forward`, passing a `ReceiverStream` of `HttpResponseMsg`. Tonic 0.12 uses the stream item as the protobuf message, so this channel is not wrapped in `Result`.

Each inbound `HttpRequestMsg` is handled on its own task:

1. Join `TARGET_URL` (trailing slash stripped) with `path`.
2. Parse the method. An unknown method becomes `GET`.
3. Copy headers except `host`, `content-length`, `transfer-encoding`, and `connection`. reqwest sets those itself and rejects duplicates.
4. Attach the body when it is non-empty.
5. On success, copy the target's status, headers, and body into `HttpResponseMsg` with the original id.
6. On a connection failure, reply with status `502` and the body `the client could not reach the target API`. That reply still races with the other workers; the gateway returns it only if it arrives first.

A new `reqwest::Client` is created per forwarded call.

## Response rule and side effects

"First response wins" is entirely the `remove` on `pending`. Replacing the oneshot with an `mpsc` receiver would let the gateway collect N replies instead. Filtering would belong either in the broadcast loop or in the worker, before it calls `TARGET_URL`.

Because the broadcast happens before any reply, every worker that successfully receives the message will call its API. A read is executed N times. A write is also executed N times. The HTTP caller still sees a single response.

## Limits

- No timeout around the oneshot. A stuck worker that stays connected holds the caller.
- Broadcast uses blocking `send` on a capacity-32 channel. A worker that stops reading delays every other recipient of that request. `try_send` would isolate it by dropping the message for that worker.
- Bodies are fully buffered, and the default gRPC message limit is unchanged.
- Header values must be UTF-8 to survive the gateway. Non-UTF-8 request headers are dropped on the way in.
- Building the HTTP response `unwrap`s. A target header the `http` crate rejects will fail that request task.
- Listen ports come from `HTTP_PORT` and `GRPC_PORT` (defaults `8080` and `50051`, both bound on `0.0.0.0`). The worker's `SERVER_ADDR` must use the same port as `GRPC_PORT`. `TARGET_URL` is still required on the worker.
- The gRPC transport is plaintext HTTP, not TLS.

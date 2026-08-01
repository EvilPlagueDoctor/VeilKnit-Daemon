# VeilKnit Android application API bridge

Android application sandboxes prevent a separately installed app from opening the daemon's private Unix-domain socket. This project therefore exposes the existing protocol-v3 daemon API through a small Binder proxy.

## Security model

The service requires:

```text
com.example.veilknit_deamon.permission.BIND_VEILKNIT_API
```

The permission uses `protectionLevel="signature"`. Only applications signed with the same certificate as the daemon can bind. Protocol-v3 registration, requested capabilities, HMAC authentication, session tokens, app-scoped DHT storage, app signing, mailbox separation, and reputation provenance continue to be enforced by the Rust daemon.

The Binder bridge does not expose the daemon's private socket, user keys, DHT writer keys, or application secrets.

## Service

```text
Package: com.example.veilknit_deamon
Class:   com.example.veilknit_deamon.VeilKnitApiService
Action:  com.example.veilknit_deamon.BIND_LOCAL_API
```

AIDL methods:

```aidl
String getDaemonStateJson();
String transact(String requestJson);
long subscribe(String requestJson, IVeilKnitStreamCallback callback);
void unsubscribe(long subscriptionId);
```

`transact` sends one protocol-v3 request and returns one response. `subscribe` keeps a socket open and forwards each streamed JSON line through the callback.

## Build/signing requirement

Debug builds made by the same Android development account normally use the same default debug certificate. Release builds of the daemon and client applications must explicitly use the same release keystore.

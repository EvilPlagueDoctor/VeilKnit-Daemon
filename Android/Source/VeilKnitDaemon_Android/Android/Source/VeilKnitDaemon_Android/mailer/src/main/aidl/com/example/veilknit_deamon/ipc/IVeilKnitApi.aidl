package com.example.veilknit_deamon.ipc;

import com.example.veilknit_deamon.ipc.IVeilKnitStreamCallback;

interface IVeilKnitApi {
    String getDaemonStateJson();
    String transact(String requestJson);
    long subscribe(String requestJson, IVeilKnitStreamCallback callback);
    void unsubscribe(long subscriptionId);
}

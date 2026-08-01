package com.example.veilknit_deamon.ipc;

interface IVeilKnitStreamCallback {
    void onLine(String line);
    void onClosed(String reason);
}

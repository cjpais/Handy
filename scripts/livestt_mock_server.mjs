#!/usr/bin/env node
import http from "node:http";
import crypto from "node:crypto";

const HOST = process.env.LIVESTT_MOCK_HOST || "127.0.0.1";
const PORT = Number(process.env.LIVESTT_MOCK_PORT || "8787");
const EXPIRE_FIRST_WS_AFTER_FIRST_BINARY =
  process.env.LIVESTT_MOCK_EXPIRE_FIRST_WS_AFTER_FIRST_BINARY === "1";
const REJECT_FIRST_WS_HANDSHAKE =
  process.env.LIVESTT_MOCK_REJECT_FIRST_WS_HANDSHAKE;
const EMIT_MULTIPLE_FINAL_CHUNKS =
  process.env.LIVESTT_MOCK_FINAL_CHUNKS === "1";

let nextSessionId = 123;
let expiredFirstWs = false;
let rejectedFirstWsHandshake = false;

function readBody(req) {
  return new Promise((resolve) => {
    const chunks = [];
    req.on("data", (chunk) => chunks.push(chunk));
    req.on("end", () => resolve(Buffer.concat(chunks)));
  });
}

function json(res, status, body) {
  const payload = JSON.stringify(body);
  res.writeHead(status, {
    "content-type": "application/json",
    "access-control-allow-origin": "*",
    "access-control-allow-credentials": "true",
    "access-control-expose-headers": "Content-Disposition",
  });
  res.end(payload);
}

function sendWsText(socket, obj) {
  const payload = Buffer.from(
    typeof obj === "string" ? obj : JSON.stringify(obj),
  );
  const header = [];
  header.push(0x81);
  if (payload.length < 126) {
    header.push(payload.length);
  } else if (payload.length < 65536) {
    header.push(126, (payload.length >> 8) & 255, payload.length & 255);
  } else {
    header.push(
      127,
      0,
      0,
      0,
      0,
      (payload.length >> 24) & 255,
      (payload.length >> 16) & 255,
      (payload.length >> 8) & 255,
      payload.length & 255,
    );
  }
  socket.write(Buffer.concat([Buffer.from(header), payload]));
}

function sendWsClose(socket, code, reason = "") {
  if (!code) {
    socket.write(Buffer.from([0x88, 0x00]));
    return;
  }

  const reasonBytes = Buffer.from(reason);
  const payload = Buffer.alloc(2 + reasonBytes.length);
  payload.writeUInt16BE(code, 0);
  reasonBytes.copy(payload, 2);
  const header = [0x88];
  if (payload.length < 126) {
    header.push(payload.length);
  } else {
    header.push(126, (payload.length >> 8) & 255, payload.length & 255);
  }
  socket.write(Buffer.concat([Buffer.from(header), payload]));
}

function parseFrames(buffer) {
  const frames = [];
  let offset = 0;

  while (offset + 2 <= buffer.length) {
    const first = buffer[offset++];
    const second = buffer[offset++];
    const opcode = first & 0x0f;
    const masked = Boolean(second & 0x80);
    let len = second & 0x7f;

    if (len === 126) {
      if (offset + 2 > buffer.length) break;
      len = buffer.readUInt16BE(offset);
      offset += 2;
    } else if (len === 127) {
      if (offset + 8 > buffer.length) break;
      const high = buffer.readUInt32BE(offset);
      const low = buffer.readUInt32BE(offset + 4);
      offset += 8;
      len = high * 2 ** 32 + low;
    }

    let mask;
    if (masked) {
      if (offset + 4 > buffer.length) break;
      mask = buffer.subarray(offset, offset + 4);
      offset += 4;
    }

    if (offset + len > buffer.length) break;
    let payload = buffer.subarray(offset, offset + len);
    offset += len;

    if (masked && mask) {
      const unmasked = Buffer.alloc(payload.length);
      for (let i = 0; i < payload.length; i++) {
        unmasked[i] = payload[i] ^ mask[i % 4];
      }
      payload = unmasked;
    }

    frames.push({ opcode, payload });
  }

  return { frames, rest: buffer.subarray(offset) };
}

const server = http.createServer(async (req, res) => {
  const url = new URL(req.url, `http://${req.headers.host}`);

  if (req.method === "OPTIONS") {
    res.writeHead(204, {
      "access-control-allow-origin": "*",
      "access-control-allow-credentials": "true",
      "access-control-allow-methods": "POST, OPTIONS",
      "access-control-allow-headers": "content-type, authorization",
    });
    res.end();
    return;
  }

  if (req.method === "POST" && url.pathname === "/auth/login") {
    const body = await readBody(req);
    const contentType = req.headers["content-type"] || "";

    let username = "";
    if (contentType.includes("application/x-www-form-urlencoded")) {
      const params = new URLSearchParams(body.toString("utf8"));
      username = params.get("username") || "";
    } else if (contentType.includes("multipart/form-data")) {
      const text = body.toString("utf8");
      const m = text.match(/name="username"\r?\n\r?\n([^\r\n]+)/);
      username = m ? m[1] : "";
    }

    console.log(
      `[auth] POST /auth/login content-type=${contentType} username_present=${Boolean(username)}`,
    );
    json(res, 200, {
      access_token: "mock-access-token",
      refresh_token: "mock-refresh-token",
    });
    return;
  }

  if (req.method === "POST" && url.pathname === "/auth/refresh") {
    const body = await readBody(req);
    const contentType = req.headers["content-type"] || "";

    let refreshTokenPresent = false;
    if (contentType.includes("application/json")) {
      try {
        const parsed = JSON.parse(body.toString("utf8"));
        refreshTokenPresent =
          typeof parsed.refresh_token === "string" &&
          parsed.refresh_token.trim().length > 0;
      } catch {
        refreshTokenPresent = false;
      }
    }

    console.log(
      `[auth] POST /auth/refresh content-type=${contentType} refresh_token_present=${refreshTokenPresent}`,
    );

    if (!refreshTokenPresent) {
      json(res, 401, { detail: "refresh token required" });
      return;
    }

    json(res, 200, {
      access_token: "mock-access-token-refreshed",
      refresh_token: "mock-refresh-token-refreshed",
    });
    return;
  }

  json(res, 404, { detail: "Not found" });
});

server.on("upgrade", (req, socket) => {
  const url = new URL(req.url, `http://${req.headers.host}`);

  if (url.pathname !== "/api/ws/live-transcription") {
    console.log(`[ws] rejected path=${url.pathname}`);
    socket.destroy();
    return;
  }

  if (
    !rejectedFirstWsHandshake &&
    (REJECT_FIRST_WS_HANDSHAKE === "401" || REJECT_FIRST_WS_HANDSHAKE === "403")
  ) {
    rejectedFirstWsHandshake = true;
    const statusText =
      REJECT_FIRST_WS_HANDSHAKE === "401" ? "Unauthorized" : "Forbidden";
    console.log(
      `[ws] simulating first handshake rejection status=${REJECT_FIRST_WS_HANDSHAKE}`,
    );
    socket.write(
      `HTTP/1.1 ${REJECT_FIRST_WS_HANDSHAKE} ${statusText}\r\n` +
        "Connection: close\r\n" +
        "Content-Length: 0\r\n" +
        "\r\n",
    );
    socket.end();
    return;
  }

  const token = url.searchParams.get("token");
  const audioFormat = url.searchParams.get("audio_format") || "webm_opus";
  const consultationId = url.searchParams.get("consultation_id");

  if (!token) {
    console.log("[ws] rejected missing token");
    socket.destroy();
    return;
  }

  if (audioFormat !== "pcm") {
    console.log(`[ws] rejected unsupported audio_format=${audioFormat}`);
    socket.destroy();
    return;
  }

  const key = req.headers["sec-websocket-key"];
  const accept = crypto
    .createHash("sha1")
    .update(key + "258EAFA5-E914-47DA-95CA-C5AB0DC85B11")
    .digest("base64");

  socket.write(
    "HTTP/1.1 101 Switching Protocols\r\n" +
      "Upgrade: websocket\r\n" +
      "Connection: Upgrade\r\n" +
      `Sec-WebSocket-Accept: ${accept}\r\n` +
      "\r\n",
  );

  const sessionId = nextSessionId++;
  let binaryFrames = 0;
  let binaryBytes = 0;
  let buffer = Buffer.alloc(0);
  let sessionEndedSent = false;

  console.log(
    `[ws] connected session=${sessionId} audio_format=${audioFormat} consultation_id=${consultationId || "(none)"}`,
  );

  sendWsText(socket, { type: "session_started", session_id: sessionId });

  socket.on("data", (chunk) => {
    buffer = Buffer.concat([buffer, chunk]);
    const parsed = parseFrames(buffer);
    buffer = parsed.rest;

    for (const frame of parsed.frames) {
      if (frame.opcode === 0x2) {
        binaryFrames++;
        binaryBytes += frame.payload.length;
        console.log(
          `[ws] binary frame #${binaryFrames}, bytes=${frame.payload.length}, total=${binaryBytes}`,
        );

        if (binaryFrames === 1) {
          if (EXPIRE_FIRST_WS_AFTER_FIRST_BINARY && !expiredFirstWs) {
            expiredFirstWs = true;
            console.log("[ws] simulating auth expiration close code=4001");
            sendWsClose(socket, 4001, "token expired");
            socket.end();
            return;
          }

          sendWsText(socket, {
            type: "partial",
            session_id: sessionId,
            text: "mock partial text",
            is_final: false,
          });
        }
      } else if (frame.opcode === 0x1) {
        const text = frame.payload.toString("utf8");
        console.log(`[ws] text frame: ${text}`);

        let isStop = text === "stop_record";
        try {
          const parsedText = JSON.parse(text);
          isStop = isStop || parsedText.type === "stop_record";
        } catch {}

        if (isStop) {
          console.log(
            `[ws] stop_record received after frames=${binaryFrames}, bytes=${binaryBytes}`,
          );
          if (EMIT_MULTIPLE_FINAL_CHUNKS) {
            sendWsText(socket, {
              type: "final",
              session_id: sessionId,
              text: "mock",
              is_final: true,
              start_time: 0.0,
              end_time: 0.5,
            });
            sendWsText(socket, {
              type: "final",
              session_id: sessionId,
              text: "final text",
              is_final: true,
              start_time: 0.5,
              end_time: 1.0,
            });
          } else {
            sendWsText(socket, {
              type: "final",
              session_id: sessionId,
              text: "mock final text",
              is_final: true,
              start_time: 0.0,
              end_time: 1.0,
            });
          }
          sendWsText(socket, { type: "session_ended", session_id: sessionId });
          sessionEndedSent = true;
          console.log(
            "[ws] final and session_ended sent; waiting for client close",
          );
        }
      } else if (frame.opcode === 0x8) {
        console.log("[ws] client close frame received");
        sendWsClose(socket);
        socket.end();
      }
    }
  });

  socket.on("close", () => {
    console.log(
      `[ws] client closed status=${
        sessionEndedSent ? "after_session_ended" : "before_session_ended"
      } session=${sessionId} frames=${binaryFrames} bytes=${binaryBytes}`,
    );
  });

  socket.on("error", (err) => {
    console.log(`[ws] error: ${err.message}`);
  });
});

server.listen(PORT, HOST, () => {
  console.log(`LiveSTT mock server listening on http://${HOST}:${PORT}`);
  console.log("Auth endpoint: POST /auth/login");
  console.log("WebSocket endpoint: /api/ws/live-transcription");
});

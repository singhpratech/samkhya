#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""End-to-end tests for the standalone and primary Python wire transports."""

from __future__ import annotations

import argparse
import http.client
import json
import socket
import subprocess
import sys
import time
import urllib.error
import urllib.request
from pathlib import Path
from typing import Optional


HOST = "127.0.0.1"
DUMMY_SCRIPT = Path(__file__).with_name("llm_dummy_backend.py")
PRIMARY_SCRIPT = Path(__file__).with_name("llm_infer_server.py")


def available_port() -> int:
    with socket.socket() as sock:
        sock.bind((HOST, 0))
        return int(sock.getsockname()[1])


def request(
    base_url: str, path: str, *, method: str = "GET", body: Optional[bytes] = None
) -> tuple[int, bytes]:
    headers = {"content-type": "application/json"} if body is not None else {}
    req = urllib.request.Request(
        f"{base_url}{path}", data=body, headers=headers, method=method
    )
    try:
        with urllib.request.urlopen(req, timeout=2) as response:
            return int(response.status), response.read()
    except urllib.error.HTTPError as exc:
        return int(exc.code), exc.read()


def post_json(base_url: str, value: object) -> tuple[int, bytes]:
    return request(
        base_url,
        "/infer",
        method="POST",
        body=json.dumps(value).encode("utf-8"),
    )


def wait_until_ready(base_url: str, process: subprocess.Popen) -> None:
    for _ in range(80):
        if process.poll() is not None:
            stderr = process.stderr.read() if process.stderr else ""
            raise RuntimeError(f"server exited with {process.returncode}: {stderr}")
        try:
            status, _ = request(base_url, "/health")
            if status == 200:
                return
        except OSError:
            pass
        time.sleep(0.05)
    raise TimeoutError("Python dummy server did not become ready")


def chunked_body(port: int, byte_length: int) -> int:
    """Send a headerless body of exactly ``byte_length`` bytes."""
    connection = http.client.HTTPConnection(HOST, port, timeout=5)
    try:
        connection.putrequest("POST", "/infer")
        connection.putheader("content-type", "application/json")
        connection.putheader("transfer-encoding", "chunked")
        connection.endheaders()
        remaining = byte_length
        while remaining:
            chunk = b" " * min(remaining, 1024 * 1024)
            encoded_len = f"{len(chunk):x}\r\n".encode("ascii")
            connection.send(encoded_len)
            connection.send(chunk)
            connection.send(b"\r\n")
            remaining -= len(chunk)
        connection.send(b"0\r\n\r\n")
        response = connection.getresponse()
        response.read()
        return int(response.status)
    finally:
        connection.close()


def malformed_chunk_size(port: int) -> int:
    """Send a signed chunk size, which is not valid HTTP chunk grammar."""
    connection = http.client.HTTPConnection(HOST, port, timeout=5)
    try:
        connection.putrequest("POST", "/infer")
        connection.putheader("content-type", "application/json")
        connection.putheader("transfer-encoding", "chunked")
        connection.endheaders()
        connection.send(b"-1\r\n")
        response = connection.getresponse()
        response.read()
        return int(response.status)
    finally:
        connection.close()


def exercise(
    script: Path,
    label: str,
    extra_args: list[str],
    *,
    test_chunked: bool,
    test_chunk_framing: bool,
) -> None:
    port = available_port()
    base_url = f"http://{HOST}:{port}"
    process = subprocess.Popen(
        [
            sys.executable,
            str(script),
            "--host",
            HOST,
            "--port",
            str(port),
            *extra_args,
        ],
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
    )
    try:
        wait_until_ready(base_url, process)

        status, raw = request(base_url, "/health")
        assert status == 200
        health = json.loads(raw)
        assert health["ok"] is True and health["backend"] == "dummy"

        status, raw = post_json(
            base_url,
            {"features": [1, 2, 3, 4, 5, 6, 7], "baseline_estimate": 42},
        )
        valid = json.loads(raw)
        assert status == 200 and valid["estimate"] == 42
        if "_latency_ms" in valid:
            assert isinstance(valid["_latency_ms"], (int, float))
            assert valid["_latency_ms"] >= 0

        status, _ = post_json(
            base_url, {"features": [1, 2], "baseline_estimate": 42}
        )
        assert status == 400

        status, _ = post_json(
            base_url,
            {"features": [1, 2, 3, 4, 5, 6, "7"], "baseline_estimate": 42},
        )
        assert status == 400

        status, _ = post_json(
            base_url,
            {"features": [1, 2, 3, 4, 5, 6, 7], "baseline_estimate": 1 << 64},
        )
        assert status == 400

        status, raw = post_json(
            base_url,
            {"features": [1, 2, 3, 4, 5, 6, 7], "baseline_estimate": (1 << 64) - 1},
        )
        assert status == 200 and json.loads(raw)["estimate"] == (1 << 64) - 1

        status, _ = request(base_url, "/infer", method="POST", body=b"{")
        assert status == 400

        for non_object in (None, [], 42):
            status, _ = post_json(base_url, non_object)
            assert status == 400

        status, _ = post_json(
            base_url,
            {"features": [0] * (7 * 1025), "baseline_estimate": 1},
        )
        assert status == 413

        if test_chunked:
            assert chunked_body(port, 8 * 1024 * 1024) == 400
            assert chunked_body(port, 8 * 1024 * 1024 + 1) == 413
        if test_chunk_framing:
            assert malformed_chunk_size(port) == 400

        status, _ = request(base_url, "/missing")
        assert status == 404
    finally:
        process.terminate()
        try:
            process.wait(timeout=5)
        except subprocess.TimeoutExpired:
            process.kill()
            process.wait(timeout=5)

    print(f"wire contract ok: {label}")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--primary",
        action="store_true",
        help="test the FastAPI primary server instead of the stdlib standalone server",
    )
    args = parser.parse_args()

    if args.primary:
        exercise(
            PRIMARY_SCRIPT,
            "Python primary dummy backend",
            ["--backend", "dummy"],
            test_chunked=True,
            test_chunk_framing=False,
        )
    else:
        exercise(
            DUMMY_SCRIPT,
            "Python standalone dummy",
            [],
            test_chunked=True,
            test_chunk_framing=True,
        )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

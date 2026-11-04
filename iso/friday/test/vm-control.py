#!/usr/bin/env python3
"""
FRIDAY OS VM Terminal Control & Real-time Monitoring Tool
Communicates directly with the QEMU VM via:
  1. Serial console socket (/tmp/friday-qemu-serial.sock)
  2. QEMU Monitor socket (/tmp/friday-qemu-monitor.sock)
  3. Guest SSH daemon (localhost:2222)
  4. Framebuffer screenshots & serial logs
"""

import argparse
import os
import select
import socket
import subprocess
import sys
import time

MONITOR_SOCK = "/tmp/friday-qemu-monitor.sock"
SERIAL_SOCK = "/tmp/friday-qemu-serial.sock"
SERIAL_LOG = "/tmp/friday-qemu-serial.log"
SSH_HOST = "127.0.0.1"
SSH_PORT = 2222


def send_qemu_monitor(cmd: str, timeout: float = 2.0) -> str:
    if not os.path.exists(MONITOR_SOCK):
        return f"Error: QEMU monitor socket {MONITOR_SOCK} not found (VM not running?)."
    try:
        s = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        s.settimeout(timeout)
        s.connect(MONITOR_SOCK)
        # Drain initial banner / prompt
        s.settimeout(0.5)
        try:
            while True:
                data = s.recv(1024)
                if not data or b"(qemu)" in data:
                    break
        except TimeoutError:
            pass

        s.sendall((cmd + "\n").encode("utf-8"))
        response = ""
        s.settimeout(timeout)
        while True:
            try:
                chunk = s.recv(4096).decode("utf-8", errors="replace")
                if not chunk:
                    break
                response += chunk
                if "(qemu)" in response:
                    break
            except TimeoutError:
                break
        s.close()
        # Clean up echo and final prompt
        lines = [
            line
            for line in response.splitlines()
            if not line.strip().startswith("(qemu)") and line.strip() != cmd.strip()
        ]
        return "\n".join(lines).strip()
    except Exception as e:
        return f"Error communicating with QEMU monitor: {e}"


def send_serial(cmd: str, wait_secs: float = 2.0) -> str:
    if not os.path.exists(SERIAL_SOCK):
        return f"Error: Serial socket {SERIAL_SOCK} not found."
    try:
        s = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        s.settimeout(1.0)
        s.connect(SERIAL_SOCK)
        s.sendall((cmd + "\n").encode("utf-8"))
        time.sleep(wait_secs)
        response = ""
        while True:
            ready, _, _ = select.select([s], [], [], 0.5)
            if ready:
                data = s.recv(4096).decode("utf-8", errors="replace")
                if not data:
                    break
                response += data
            else:
                break
        s.close()
        return response.strip()
    except Exception as e:
        return f"Error sending serial command: {e}"


def ssh_exec(cmd: str, timeout: int = 30) -> subprocess.CompletedProcess[str]:
    ssh_cmd = [
        "ssh",
        "-p",
        str(SSH_PORT),
        "-o",
        "LogLevel=ERROR",
        "-o",
        "StrictHostKeyChecking=no",
        "-o",
        "UserKnownHostsFile=/dev/null",
        "-o",
        "ConnectTimeout=5",
        "-o",
        "BatchMode=yes",
        f"root@{SSH_HOST}",
        cmd,
    ]
    return subprocess.run(ssh_cmd, capture_output=True, text=True, timeout=timeout)


def check_status() -> None:
    print("=== FRIDAY OS VM Status ===")
    # Check if QEMU process is alive
    res = subprocess.run(["pgrep", "-f", "qemu-system-x86_64"], capture_output=True, text=True)
    pids = res.stdout.strip().split()
    if pids and pids[0]:
        print(f"● QEMU Status: RUNNING (PID: {', '.join(pids)})")
    else:
        print("○ QEMU Status: STOPPED")
        return

    # Check monitor
    mon_res = send_qemu_monitor("info status")
    print(f"● QEMU Engine: {mon_res}")

    # Check SSH Port
    sock = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    sock.settimeout(1.0)
    ssh_open = sock.connect_ex((SSH_HOST, SSH_PORT)) == 0
    sock.close()
    print(
        f"● SSH Daemon (127.0.0.1:{SSH_PORT}): "
        + ("ONLINE (Ready for commands)" if ssh_open else "BOOTING / INITIALIZING")
    )

    # Check serial log size
    if os.path.exists(SERIAL_LOG):
        size = os.path.getsize(SERIAL_LOG)
        print(f"● Serial Log: {SERIAL_LOG} ({size} bytes)")
    print("===========================")


def wait_ready(timeout: int = 180) -> bool:
    print(f"Waiting for VM to boot and present console / SSH (timeout {timeout}s)...")
    start = time.time()
    while time.time() - start < timeout:
        # Check SSH
        sock = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        sock.settimeout(0.5)
        if sock.connect_ex((SSH_HOST, SSH_PORT)) == 0:
            sock.close()
            print(f"VM SSH online at 127.0.0.1:{SSH_PORT} after {int(time.time() - start)}s!")
            return True
        sock.close()

        # Check serial log for prompt
        if os.path.exists(SERIAL_LOG):
            with open(SERIAL_LOG, errors="replace") as f:
                content = f.read()
                if (
                    "login:" in content
                    or "root@" in content
                    or "archiso ~ #" in content
                    or "Startup finished" in content
                ):
                    print(f"VM console ready after {int(time.time() - start)}s!")
                    return True
        time.sleep(2)
    print("Timed out waiting for VM readiness.")
    return False


def screenshot(out_path: str = "screenshot.png") -> None:
    ppm_path = "/tmp/friday-screen.ppm"
    print("Triggering QEMU screendump...")
    send_qemu_monitor(f"screendump {ppm_path}")
    time.sleep(0.5)
    if os.path.exists(ppm_path):
        subprocess.run(["ffmpeg", "-y", "-i", ppm_path, out_path], capture_output=True)
        print(f"Screenshot saved to {out_path}")
        try:
            os.remove(ppm_path)
        except Exception:
            pass
    else:
        print("Failed to capture framebuffer dump from QEMU monitor.")


def main() -> None:
    parser = argparse.ArgumentParser(description="FRIDAY OS VM Terminal Control & Monitoring")
    subparsers = parser.add_subparsers(dest="action")

    subparsers.add_parser("status", help="Check VM and services status")

    exec_p = subparsers.add_parser("exec", help="Execute command inside VM (via SSH or Serial)")
    exec_p.add_argument("command", help="Command to run in VM")
    exec_p.add_argument(
        "--serial", action="store_true", help="Force serial transport instead of SSH"
    )

    ser_p = subparsers.add_parser("serial", help="Send command to VM serial console")
    ser_p.add_argument("command", help="Command to send")
    ser_p.add_argument("--wait", type=float, default=2.0, help="Wait time in seconds")

    mon_p = subparsers.add_parser("monitor", help="Send command to QEMU monitor")
    mon_p.add_argument("command", help="QEMU monitor command (e.g. info status, quit)")

    shot_p = subparsers.add_parser("screenshot", help="Capture VM framebuffer to PNG")
    shot_p.add_argument("--output", default="/tmp/friday-screen.png", help="Path to save PNG")

    tail_p = subparsers.add_parser("logs", help="Tail VM serial logs")
    tail_p.add_argument("-n", type=int, default=50, help="Number of lines")

    wait_p = subparsers.add_parser("wait-ready", help="Wait for VM to complete booting")
    wait_p.add_argument("--timeout", type=int, default=180, help="Timeout in seconds")

    subparsers.add_parser("stop", help="Stop VM cleanly")

    args = parser.parse_args()

    if args.action == "status":
        check_status()
    elif args.action == "exec":
        if not args.serial:
            try:
                res = ssh_exec(args.command)
                if res.returncode == 0:
                    sys.stdout.write(res.stdout)
                    sys.stderr.write(res.stderr)
                    sys.exit(0)
                else:
                    print(
                        f"SSH returned {res.returncode}: {res.stderr.strip()}. "
                        "Falling back to serial console..."
                    )
            except Exception as e:
                print(f"SSH failed ({e}). Falling back to serial console...")
        out = send_serial(args.command)
        print(out)
    elif args.action == "serial":
        out = send_serial(args.command, wait_secs=args.wait)
        print(out)
    elif args.action == "monitor":
        out = send_qemu_monitor(args.command)
        print(out)
    elif args.action == "screenshot":
        screenshot(args.output)
    elif args.action == "logs":
        if os.path.exists(SERIAL_LOG):
            subprocess.run(["tail", "-n", str(args.n), SERIAL_LOG])
        else:
            print("No serial log file found.")
    elif args.action == "wait-ready":
        ready = wait_ready(args.timeout)
        sys.exit(0 if ready else 1)
    elif args.action == "stop":
        print("Sending powerdown to QEMU VM...")
        print(send_qemu_monitor("system_powerdown"))
    else:
        parser.print_help()


if __name__ == "__main__":
    main()

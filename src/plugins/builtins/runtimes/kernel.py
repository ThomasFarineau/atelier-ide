"""Bridge between Atelier and a Jupyter kernel, run by the Python of the project.

Atelier writes one JSON request per line on the standard input ({"op": "execute", "cell": 3,
"code": "..."} or {"op": "interrupt"}) and reads one JSON event per line on the standard output.
Closing the standard input shuts the kernel down.
"""

import json
import queue
import sys
import threading


def send(**event):
    sys.stdout.write(json.dumps(event, default=str) + "\n")
    sys.stdout.flush()


try:
    from jupyter_client.manager import KernelManager
except ImportError as error:
    send(type="missing", error=str(error))
    sys.exit(0)

manager = KernelManager(kernel_name="python3")
try:
    manager.start_kernel(cwd=sys.argv[1] if len(sys.argv) > 1 else None)
except Exception as error:  # No ipykernel: no kernel named python3.
    send(type="missing", error=str(error))
    sys.exit(0)

client = manager.client()
client.start_channels()
try:
    client.wait_for_ready(timeout=120)
except RuntimeError as error:
    send(type="failed", error=str(error))
    manager.shutdown_kernel(now=True)
    sys.exit(1)

send(type="ready", version=sys.version.split()[0])

requests = queue.Queue()


def read():
    for line in sys.stdin:
        try:
            request = json.loads(line)
        except ValueError:
            continue
        if request.get("op") == "interrupt":
            manager.interrupt_kernel()
        else:
            requests.put(request)
    requests.put(None)


threading.Thread(target=read, daemon=True).start()

OUTPUTS = ("stream", "display_data", "execute_result", "error", "clear_output")

while True:
    request = requests.get()
    if request is None:
        break
    cell = request.get("cell")
    msg_id = client.execute(request.get("code", ""), allow_stdin=False)
    count = None
    while True:
        try:
            message = client.get_iopub_msg(timeout=1)
        except queue.Empty:
            if not manager.is_alive():
                send(type="failed", error="kernel died")
                break
            continue
        if message["parent_header"].get("msg_id") != msg_id:
            continue
        kind = message["msg_type"]
        content = message["content"]
        if kind == "status" and content.get("execution_state") == "idle":
            break
        if kind == "execute_input":
            count = content.get("execution_count")
        elif kind in OUTPUTS:
            send(type="output", cell=cell, kind=kind, content=content)
    send(type="done", cell=cell, count=count)

client.stop_channels()
manager.shutdown_kernel(now=True)

"""Local TLS registry fixture. The HTTP target records any downgrade request."""
import http.server
import json
import pathlib
import ssl
import sys
import threading

root = pathlib.Path(sys.argv[1])


class Handler(http.server.BaseHTTPRequestHandler):
    def log_message(self, *args):
        pass

    def do_GET(self):
        if self.server is https:
            self.send_response(302)
            self.send_header("Location", f"http://127.0.0.1:{http.server_port}/release")
            self.end_headers()
        else:
            (root / "downgraded").write_text("yes")
            body = json.dumps({"version": "9.9.9", "url": "https://example.org/agy", "sha512": "a" * 128}).encode()
            self.send_response(200)
            self.end_headers()
            self.wfile.write(body)


http = http.server.HTTPServer(("127.0.0.1", 0), Handler)
https = type(http)(("127.0.0.1", 0), Handler)
context = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
context.load_cert_chain(root / "cert.pem", root / "key.pem")
https.socket = context.wrap_socket(https.socket, server_side=True)
threading.Thread(target=http.serve_forever, daemon=True).start()
print(https.server_port, flush=True)
https.serve_forever()

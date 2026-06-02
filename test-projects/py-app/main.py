from http.server import HTTPServer, BaseHTTPRequestHandler

class Handler(BaseHTTPRequestHandler):
    def do_GET(self):
        self.send_response(200)

if __name__ == "__main__":
    server = HTTPServer(("0.0.0.0", 8000), Handler)
    print("http://localhost:8000", flush=True)
    server.serve_forever()

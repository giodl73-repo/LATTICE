"""Serve the selected test build without interpolating its path into a shell."""
from functools import partial
from http.server import SimpleHTTPRequestHandler, ThreadingHTTPServer
import os
from pathlib import Path

if __name__ == '__main__':
    directory = Path(os.environ.get('LATTICE_DIST', Path(__file__).resolve().parents[1]/'dist')).resolve()
    with ThreadingHTTPServer(('127.0.0.1', 8771), partial(SimpleHTTPRequestHandler, directory=str(directory))) as server:
        server.serve_forever()

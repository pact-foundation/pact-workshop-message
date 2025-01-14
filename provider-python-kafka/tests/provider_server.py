"""
HTTP Server to route message requests to message producer function.
"""

from __future__ import annotations

import logging
import re
import signal
import socket
import subprocess
import sys
import time
from contextlib import closing, contextmanager
from importlib import import_module
from pathlib import Path
from threading import Thread
from typing import TYPE_CHECKING, NoReturn
import json
import requests
import base64
sys.path.append(str(Path(__file__).parent.parent))

from yarl import URL

import flask

if TYPE_CHECKING:
    from collections.abc import Generator

logger = logging.getLogger(__name__)


class Provider:
    """
    Provider class to route message requests to message producer function.

    Sets up three endpoints:
        - /_test/ping: A simple ping endpoint for testing.
        - /produce_message: Route message requests to the handler function.
        - /set_provider_state: Set the provider state.

    The specific `produce_message` and `set_provider_state` URLs can be configured
    with the `produce_message_url` and `set_provider_state_url` arguments.
    """

    def __init__(  # noqa: PLR0913
        self,
        handler_module: str,
        handler_function: str,
        produce_message_url: str,
        state_provider_module: str,
        state_provider_function: str,
        set_provider_state_url: str,
    ) -> None:
        """
        Initialize the provider.

        Args:
            handler_module:
                The name of the module containing the handler function.
            handler_function:
                The name of the handler function.
            produce_message_url:
                The URL to route message requests to the handler function.
            state_provider_module:
                The name of the module containing the state provider setup function.
            state_provider_function:
                The name of the state provider setup function.
            set_provider_state_url:
                The URL to set the provider state.
        """
        self.app = flask.Flask("Provider")
        self.handler_function = getattr(import_module(handler_module), handler_function)
        self.produce_message_url = produce_message_url
        self.set_provider_state_url = set_provider_state_url
        if state_provider_module:
            self.state_provider_function = getattr(
                import_module(state_provider_module), state_provider_function
            )

        @self.app.get("/_test/ping")
        def ping() -> str:
            """Simple ping endpoint for testing."""
            return "pong"

        @self.app.route(self.produce_message_url, methods=["POST"])
        def produce_message() -> flask.Response | tuple[str, int]:
            """
            Route a message request to the handler function.

            Returns:
                The response from the handler function.
            """
            try:
                request_data = flask.request.get_json()
                description = request_data.get("description")
                if self.state_provider_function:
                    self.state_provider_function(description)
                body, content_type, metadata = self.handler_function()
                if metadata:
                    metadata_str = base64.b64encode(metadata.encode('utf-8')).decode('utf-8')
                    headers = {"pact-message-metadata": metadata_str}
                else:
                    headers = {}
                return flask.Response(
                    response=body,
                    status=200,
                    content_type=content_type,
                    headers=headers,
                    direct_passthrough=True,
                )
            except Exception as e:  # noqa: BLE001
                return str(e), 500

        @self.app.route(self.set_provider_state_url, methods=["POST"])
        def set_provider_state() -> tuple[str, int]:
            """
            Calls the state provider function with the state provided in the request.

            Returns:
                A response indicating that the state has been set.
            """
            if self.state_provider_function:
                self.state_provider_function(flask.request.args["state"])
            return "Provider state set", 200

    def _find_free_port(self) -> int:
        """
        Find a free port.

        This is used to find a free port to host the API on when running locally. It
        is allocated, and then released immediately so that it can be used by the
        API.

        Returns:
            The port number.
        """
        with closing(socket.socket(socket.AF_INET, socket.SOCK_STREAM)) as s:
            s.bind(("", 0))
            s.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
            return s.getsockname()[1]

    def run(self) -> None:
        """
        Start the provider.
        """
        url = URL(f"http://localhost:{self._find_free_port()}")
        sys.stderr.write(f"Starting provider on {url}\n")

        self.app.run(
            host=url.host,
            port=url.port,
            debug=True,
        )


@contextmanager
def start_provider(**kwargs: str) -> Generator[URL, None, None]:  # noqa: C901
    """
    Start the provider app.

    Expects kwargs to to contain the following:
        handler_module: Required. The name of the module containing
                        the handler function.
        handler_function: Required. The name of the handler function.
        produce_message_url: Optional. The URL to route message requests to
                             the handler function.
        state_provider_module: Optional. The name of the module containing
                               the state provider setup function.
        state_provider_function: Optional. The name of the state provider
                                 setup function.
        set_provider_state_url: Optional. The URL to set the provider state.
    """
    for arg, value in kwargs.items():
        print(f"{arg}: {value}")
    process = subprocess.Popen(  # noqa: S603
        [
            sys.executable,
            Path(__file__),
            kwargs.pop("handler_module"),
            kwargs.pop("handler_function"),
            kwargs.pop("produce_message_url", "/produce_message"),
            kwargs.pop("state_provider_module", ""),
            kwargs.pop("state_provider_function", ""),
            kwargs.pop("set_provider_state_url", "/set_provider_state"),
        ],
        cwd=Path.cwd(),
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        encoding="utf-8",
    )

    pattern = re.compile(r" \* Running on (?P<url>[^ ]+)")
    while True:
        if process.poll() is not None:
            logger.error("Provider process exited with code %d", process.returncode)
            logger.error(
                "Provider stdout: %s", process.stdout.read() if process.stdout else ""
            )
            logger.error(
                "Provider stderr: %s", process.stderr.read() if process.stderr else ""
            )
            msg = f"Provider process exited with code {process.returncode}"
            raise RuntimeError(msg)
        if (
            process.stderr
            and (line := process.stderr.readline())
            and (match := pattern.match(line))
        ):
            break
        time.sleep(0.1)

    url = URL(match.group("url"))
    logger.debug("Provider started on %s", url)
    for _ in range(50):
        try:
            response = requests.get(str(url / "_test" / "ping"), timeout=1)
            assert response.text == "pong"
            break
        except (requests.RequestException, AssertionError):
            time.sleep(0.1)
            continue
    else:
        msg = "Failed to ping provider"
        raise RuntimeError(msg)

    def redirect() -> NoReturn:
        while True:
            if process.stdout:
                while line := process.stdout.readline():
                    logger.debug("Provider stdout: %s", line.strip())
            if process.stderr:
                while line := process.stderr.readline():
                    logger.debug("Provider stderr: %s", line.strip())

    thread = Thread(target=redirect, daemon=True)
    thread.start()

    try:
        yield url
    finally:
        if sys.platform == "win32":
            process.terminate()
        else:
            process.send_signal(signal.SIGINT)


if __name__ == "__main__":
    import sys

    if len(sys.argv) < 5:
        sys.stderr.write(
            f"Usage: {sys.argv[0]} <state_provider_module> <state_provider_function> "
            f"<handler_module> <handler_function>"
        )
        sys.exit(1)

    handler_module = sys.argv[1]
    handler_function = sys.argv[2]
    produce_message_url = sys.argv[3]
    state_provider_module = sys.argv[4]
    state_provider_function = sys.argv[5]
    set_provider_state_url = sys.argv[6]
    Provider(
        handler_module,
        handler_function,
        produce_message_url,
        state_provider_module,
        state_provider_function,
        set_provider_state_url,
    ).run()
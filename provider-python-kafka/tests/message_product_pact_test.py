"""
Producer test of example message.

This test will read a pact between the message handler and the message provider
and then validate the pact against the provider.
"""

from __future__ import annotations
import pytest as pytest

from pathlib import Path
from unittest.mock import MagicMock

from server import ProductRepository
from tests.provider_server import start_provider
from pact import Verifier
import json

PACT_DIR = (Path(__file__).parent.parent.parent / "consumer-python-kafka" / "pacts").resolve()

responses: dict[str, dict[str, str]] = {
    "a product event update": {
        "function_name": "product_event_update",
    }
}

CURRENT_STATE: str | None = None
MESSAGE_DESCRIPTION: str | None = None


def message_producer_function() -> tuple[str, str, str]:
    producer = ProductRepository()
    # TODO - The demo provider_server mirrors the same bug in pact-python v2
    # incorrect mapping of states, rather than description
    assert CURRENT_STATE is not None, "Message Description is not set"
    function_name = responses.get(CURRENT_STATE, {}).get("function_name")
    assert function_name is not None, "Function name could not be found"
    if function_name == "product_event_update":
        updated_product = producer.produce_event("UPDATED",{"name": "Some Product", "type": "Product Range","version":"v1", "id":"123"})
        return (
          updated_product[1],
          "application/json",
          f'{{"kafka_topic":"{updated_product[0]}"}}'
        )

    return (
      nil,
      nil,
      nil
    )


def state_provider_function(state_name: str) -> None:
    global CURRENT_STATE  # noqa: PLW0603
    CURRENT_STATE = state_name

def test_producer() -> None:
    """
    Test the message producer.
    """
    with start_provider(
        handler_module=__name__,
        handler_function="message_producer_function",
        state_provider_module=__name__,
        state_provider_function="state_provider_function",
    ) as provider_url:
      verifier = (
          Verifier("pactflow-example-provider-python-kafka")
          .state_handler(
              f"{provider_url}/set_provider_state",
              body=False,
              teardown=True,
          )
          .add_transport(url=f"{provider_url}/produce_message")
          .add_source(PACT_DIR / "pactflow-example-consumer-python-kafka-pactflow-example-provider-python-kafka.json")
      )
      verifier.verify()
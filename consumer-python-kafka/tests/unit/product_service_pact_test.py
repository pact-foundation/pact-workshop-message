import os

import pytest as pytest
import json
import asyncio
import pytest_asyncio
from pact import Pact, match
from pathlib import Path
from typing import (
    TYPE_CHECKING,
    Any,
)
from product.product_service import receive_product_update
from product.product import Products

CONSUMER_NAME = "pactflow-example-consumer-python-kafka"
PROVIDER_NAME = os.getenv("PACT_PROVIDER", "pactflow-example-provider-python-kafka")
PACT_DIR = os.path.join(os.path.dirname(os.path.realpath(__file__)),"..","..", "pacts")

@pytest.fixture
def handler():
    return receive_product_update

@pytest.fixture(scope="module")
def pact():
    pact_dir = Path(Path(__file__).parent.parent.parent / "pacts")
    pact = Pact(CONSUMER_NAME, PROVIDER_NAME)
    yield pact.with_specification("V4")
    pact.write_file(pact_dir, overwrite=True)

@pytest.fixture
def verifier(
    handler,
):
    """
    Verifier function for the Pact.

    This function is passed to the `verify` method of the Pact object. It is
    responsible for taking in the messages (along with the context/metadata)
    and ensuring that the consumer is able to process the message correctly.

    In our case, we deserialize the message and pass it to our message
    handler for processing.
    """

    def _verifier(msg: str | bytes | None, context: dict[str, Any]) -> Products:
        assert msg is not None, "Message is None"
        data = json.loads(msg)
        print(
            "Processing message: ",
            {"input": msg, "processed_message": data, "context": context},
        )
        loop = asyncio.get_event_loop()
        result = loop.run_until_complete(handler(data))
        print("Handler result: ", result)
        return result
    yield _verifier

def test_receive_a_product_update(pact, handler, verifier):
    event = {
        "id": match.like("some-uuid-1234-5678"),
        "type": match.like("Product Range"),
        "name": match.like("Some Product"),
        "event": match.regex("UPDATED", regex=r"^(CREATED|UPDATED|DELETED)$")
    }
    (
        pact
        .upon_receiving("a product event update", "Async")
        .with_body(event,
            "application/json")
        .with_metadata({"kafka_topic": "products"})
    )
    pact.verify(verifier, "Async")
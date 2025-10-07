const {
  Matchers,
  v4AsynchronousBodyHandler,
  Pact,
} = require("@pact-foundation/pact");
const productEventHandler = require("./product.handler")
const { like, regex } = Matchers;
const path = require("path");

describe("Kafka handler", () => {
  const messagePact = new Pact({
    consumer: "pactflow-example-consumer-js-kafka",
    dir: path.resolve(process.cwd(), "pacts"),
    pactfileWriteMode: "update",
    provider: "pactflow-example-provider-js-kafka",
    logLevel: process.env.PACT_LOG_LEVEL ?? "info",
  });

  describe("receive a product update", () => {
    it("accepts a product event", () => {
      return messagePact
        .addAsynchronousInteraction()
        .expectsToReceive("a product event update", (builder) => {
          builder
            .withJSONContent({
              id: like("some-uuid-1234-5678"),
              type: like("Product Range"),
              name: like("Some Product"),
              version: like("v1"),
              event: regex("^(CREATED|UPDATED|DELETED)$", "UPDATED"),
            })
            .withMetadata({
              "contentType": "application/json",
              "kafka_topic": "products",
            });
        })
        .executeTest(v4AsynchronousBodyHandler(productEventHandler));
    });
  });
});

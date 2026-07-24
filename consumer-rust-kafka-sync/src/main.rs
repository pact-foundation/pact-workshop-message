use actix_web::{web, App, HttpResponse, HttpServer, Responder};
use futures::StreamExt;
use rdkafka::config::ClientConfig;
use rdkafka::consumer::{Consumer, StreamConsumer};
use rdkafka::message::Message;
use rdkafka::producer::{FutureProducer, FutureRecord};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Duration;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Product {
    id: String,
    r#type: String,
    name: String,
    version: String,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct ProductEvent {
    id: String,
    name: String,
    r#type: String,
    version: String,
    event: String,
}

pub struct AppState {
    products: Mutex<HashMap<String, Product>>,
}

async fn get_all(data: web::Data<AppState>) -> impl Responder {
    let products = data.products.lock().unwrap();
    let products: Vec<&Product> = products.values().collect();
    HttpResponse::Ok().json(products)
}

async fn get_by_id(data: web::Data<AppState>, product_id: web::Path<String>) -> impl Responder {
    let products = data.products.lock().unwrap();
    if let Some(product) = products.get(&product_id.into_inner()) {
        HttpResponse::Ok().json(product)
    } else {
        HttpResponse::NotFound().json("Product not found")
    }
}

pub fn product_event_processor(data: &web::Data<AppState>, payload: &[u8]) {
    let product_event: ProductEvent =
        serde_json::from_slice(payload).expect("Error deserializing product");
    let product = Product {
        id: product_event.id.clone(),
        r#type: product_event.r#type.clone(),
        name: product_event.name.clone(),
        version: product_event.version.clone(),
    };
    let mut products = data.products.lock().unwrap();
    match product_event.event.as_str() {
        "CREATED" | "UPDATED" => {
            products.insert(product_event.id.clone(), product);
        }
        "DELETED" => {
            products.remove(&product.id);
        }
        _ => {
            eprintln!("Unknown event type");
        }
    }
}

fn product_event_reply_generator(product: &Product) -> Vec<u8> {
    serde_json::to_vec(product).expect("Error serializing product")
}

async fn kafka_consumer(data: web::Data<AppState>) {
    let consumer: StreamConsumer = ClientConfig::new()
        .set("group.id", "products-group")
        .set("bootstrap.servers", "localhost:9092")
        .create()
        .expect("Consumer creation failed");

    consumer
        .subscribe(&["product_request"])
        .expect("Can't subscribe to topic");

    let mut message_stream = consumer.stream();

    while let Some(message) = message_stream.next().await {
        match message {
            Ok(m) => {
                if let Some(payload) = m.payload() {
                    product_event_processor(&data, payload);
                    // Process the request and prepare the response
                    let product_event: ProductEvent =
                        serde_json::from_slice(payload).expect("Error deserializing product");
                    println!("incoming event {:?}",product_event);
                    let product = Product {
                        id: product_event.id.clone(),
                        r#type: product_event.r#type.clone(),
                        name: product_event.name.clone(),
                        version: product_event.version.clone(),
                    };
                    let reply_payload = product_event_reply_generator(&product);
                    // Publish the response to the reply topic
                    let producer: FutureProducer = ClientConfig::new()
                        .set("bootstrap.servers", "localhost:9092")
                        .create()
                        .expect("Producer creation failed");

                    let delivery_status = producer
                        .send(
                            FutureRecord::<Vec<u8>, Vec<u8>>::to("product_reply")
                                .payload(&reply_payload),
                            Duration::from_secs(0),
                        )
                        .await;

                    match delivery_status {
                        Ok(_) => {
                            println!("Product response sent to product_reply topic");
                        }
                        Err(e) => {
                            eprintln!("Error sending product response: {:?}", e);
                        }
                    }
                }
            }
            Err(e) => eprintln!("Kafka error: {}", e),
        }
    }
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    let products = Mutex::new(HashMap::new());
    let data = web::Data::new(AppState { products });

    // Start Kafka consumer
    let data_clone = data.clone();
    actix_rt::spawn(async move {
        kafka_consumer(data_clone).await;
    });

    HttpServer::new(move || {
        App::new()
            .app_data(data.clone())
            .route("/products", web::get().to(get_all))
            .route("/products/{id}", web::get().to(get_by_id))
            .route("/product/{id}", web::get().to(get_by_id))
    })
    .bind("127.0.0.1:8080")?
    .run()
    .await
}

#[cfg(test)]
mod tests {

use expectest::{expect, prelude::be_some};
use pact_consumer::{matching_regex, prelude::*};
use pact_models::{bodies::OptionalBody, prelude::MatchingRules};
use pact_models::path_exp::DocPath;
use pact_models::prelude::{Generators, MatchingRuleCategory};
use pact_models::v4::message_parts::MessageContents;
// use pact_models::{MessageContents};
use serde_json::Value;
use crate::{product_event_processor, product_event_reply_generator, AppState};
use std::collections::HashMap;
use maplit::hashmap;
use std::sync::Mutex;
use actix_web::web;
use expectest::matchers::be_equal_to;
#[test]
fn consumes_a_product_event_update_message_and_responds() {
    // Define the Pact for the test (you can setup multiple interactions by chaining the given or message_interaction calls)
    // For messages we need to use the V4 Pact format.
    let mut pact_builder =
        // Define the message consumer and provider by name
        pact_consumer::builders::PactBuilder::new_v4("pactflow-example-consumer-rust-kafka-sync", "pactflow-example-provider-rust-kafka-sync");
    pact_builder

        // Adds an interaction given the message description and type.
        .synchronous_message_interaction("a product event update with reply", |mut i| {
            // Can set the test name (optional)
            i.test_name("consumes_a_product_event_update_message_and_responds");
            // // defines a provider state. It is optional.
            // i.given("some state");
            // // defines a provider state with parameters. It is optional.
            // i.given_with_params("some state with params {param}",&json!({
            //     "param": "some param"
            //   }));
            // Set the contents of the message. Here we use a JSON pattern, so that matching rules are applied
            i.request_json_body(json_pattern!({
              "id": like!("some-uuid-1234-5678"),
              "type": like!("Product Range"),
              "name": like!("Some Product"),
              "version": like!("v1"),
              "event": matching_regex!("^(CREATED|UPDATED|DELETED)$","UPDATED")
            }));

            // Set any required metadata
            i.request_metadata("kafka_request_topic", "product_request");


            // Setup our response

            // We would like to use our matching rules via response_json_body
            // but we cannot set metadata on responses, so we do this a different way

            // i.response_json_body(json_pattern!({
            //   "id": like!("some-uuid-1234-5678"),
            //   "type": like!("Product Range"),
            //   "name": like!("Some Product"),
            //   "version": like!("v1")
            // }));


            // TODO - multiple responses can be provided for async messages
            // only way to set metadata is using the response_contents method
            // other methods are response_json_body and response_body

            let body = json_pattern!({
                "id": like!("some-uuid-1234-5678"),
                "type": like!("Product Range"),
                "name": like!("Some Product"),
                "version": like!("v1")
                });
            let message_body = OptionalBody::Present(body.to_example().to_string().into(), Some("application/json".into()), None);
            let mut rules = MatchingRuleCategory::empty("content");
            body.extract_matching_rules(DocPath::root(), &mut rules);

            i.response_contents(&MessageContents{
                contents: message_body,
                  metadata: hashmap! {
                      "kafka_reply_topic".to_string() => serde_json::Value::String("product_reply".to_string())
                  },
                matching_rules: {
                    let mut matching_rules = MatchingRules::default();
                    matching_rules.add_rules("content", rules);
                    matching_rules
                },
                generators: Generators::default(),

            });
            // Need to return the mutated interaction builder
            i
        });

    // Arrange. setup product database
    let products = Mutex::new(HashMap::new());
    let data = web::Data::new(AppState { products });

    // This will return each message configured with the Pact builder. We need to process them
    // with out message handler (it should be the one used to actually process your messages).
    for message in pact_builder.synchronous_messages() {
        // Process the message here as it would if it came off the queue
        // the request message we must make
        let request_message_bytes = message.request.contents.value().unwrap();

        // the response message we expect to receive from the provider
        let response_message_bytes = message.response.first().unwrap().contents.value().unwrap();

        // get message metadata
        let kafka_request_topic = message.request.metadata.get("kafka_request_topic");
        // let kafka_reply_topic = message.response.metadata.get("kafka_reply_topic");

        // you may want to process the bytes into a Value
        let _request_message: Value = serde_json::from_slice(&request_message_bytes).unwrap();
        let _response_message: Value = serde_json::from_slice(&response_message_bytes).unwrap();

        // Send the message to our message processor 
        product_event_processor(&data,&request_message_bytes);

        // assert of the state of our product database, after processing the message
        let products = data.products.lock().unwrap();
        let product = products.get("some-uuid-1234-5678").unwrap();
        println!("{:?}", product);
        expect!(product.id.as_str()).to(be_equal_to("some-uuid-1234-5678".to_string()));
        expect!(product.name.clone()).to(be_equal_to("Some Product".to_string()));
        expect!(product.r#type.clone()).to(be_equal_to("Product Range".to_string()));
        expect!(product.version.clone()).to(be_equal_to("v1".to_string()));

        // assert the correct topics are included in our message
        expect!(kafka_request_topic)
            .to(be_some().value("product_request"));
        // expect!(kafka_reply_topic)
        //     .to(be_some().value("product_reply"));

        // we should now call our event reply generator and ensure it can create the appropriate message
        let actual_response: Value = serde_json::from_slice(&product_event_reply_generator(product)).unwrap();
        let expected_response: Value = serde_json::from_slice(&response_message_bytes).unwrap();
        assert_eq!(expected_response, actual_response);
    }
}

}
use actix_web::{web, App, HttpResponse, HttpServer, Responder};
use futures::TryStreamExt;
use rdkafka::config::ClientConfig;
use rdkafka::producer::{FutureProducer, FutureRecord};
use rdkafka::consumer::{StreamConsumer, Consumer};
use rdkafka::message::Message;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::time::timeout;
use std::time::Duration;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Product {
    id: Option<String>,
    name: String,
    r#type: String,
    version: Option<String>,
}

#[derive(Serialize, Deserialize)]
pub struct ProductEvent {
    id: String,
    name: String,
    r#type: String,
    version: String,
    event: String,
}

pub fn create_event(product: Product, event_type: &str) -> ProductEvent {
    let version = increment_version(product.version);
    ProductEvent {
        id: product
            .id
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string()),
        name: product.name,
        r#type: product.r#type,
        event: event_type.to_string(),
        version,
    }
}
fn increment_version(version: Option<String>) -> String {
    match version {
        Some(v) => {
            let num: u32 = v[1..].parse().unwrap();
            format!("v{}", num + 1)
        }
        None => "v1".to_string(),
    }
}

async fn create_product(
    product: web::Json<Product>,
    producer: web::Data<Arc<FutureProducer>>,
    consumer: web::Data<Arc<StreamConsumer>>,
) -> impl Responder {
    let request_topic = "product_request";
    let correlation_id = uuid::Uuid::new_v4().to_string();
    let event = create_event(product.into_inner(),"CREATED");
    let payload = serde_json::to_string(&event).unwrap();
    let record = FutureRecord::to(request_topic)
        .key(&correlation_id)
        .payload(&payload);
    println!("sending message {}", payload);
    producer.send(record, Duration::from_secs(0)).await.unwrap();

    let mut stream = consumer.stream();
    'outer: while let Some(message) = match timeout(Duration::from_secs(5), stream.try_next()).await {
        Ok(Ok(msg)) => msg,
        Ok(Err(_)) | Err(_) => return HttpResponse::InternalServerError().body("Failed to get response"),
    } {
        if let Some(payload) = message.payload() {
            consumer.commit_message(&message, rdkafka::consumer::CommitMode::Async).unwrap();
            let response: Product = serde_json::from_slice(payload).unwrap();
            return HttpResponse::Ok().json(response);
        } else {
            continue 'outer;
        }
    }

    HttpResponse::InternalServerError().body("Failed to get response")
}
async fn update_product(
    product: web::Json<Product>,
    producer: web::Data<Arc<FutureProducer>>,
    consumer: web::Data<Arc<StreamConsumer>>,
) -> impl Responder {
    let request_topic = "product_request";
    let correlation_id = uuid::Uuid::new_v4().to_string();
    let event = create_event(product.into_inner(),"UPDATED");

    let payload = serde_json::to_string(&event).unwrap();
    let record = FutureRecord::to(request_topic)
        .key(&correlation_id)
        .payload(&payload);
    println!("sending message {}", payload);
    producer.send(record, Duration::from_secs(0)).await.unwrap();

    let mut stream = consumer.stream();
    'outer: while let Some(message) = match timeout(Duration::from_secs(5), stream.try_next()).await {
        Ok(Ok(msg)) => msg,
        Ok(Err(_)) | Err(_) => return HttpResponse::InternalServerError().body("Failed to get response"),
    } {
        if let Some(payload) = message.payload() {
            consumer.commit_message(&message, rdkafka::consumer::CommitMode::Async).unwrap();
            let response: Product = serde_json::from_slice(payload).unwrap();
            return HttpResponse::Ok().json(response);
        } else {
            continue 'outer;
        }
    }

    HttpResponse::InternalServerError().body("Failed to get response")
}
async fn delete_product(
    product: web::Json<Product>,
    producer: web::Data<Arc<FutureProducer>>,
    consumer: web::Data<Arc<StreamConsumer>>,
) -> impl Responder {
    let request_topic = "product_request";
    let correlation_id = uuid::Uuid::new_v4().to_string();
    let event = create_event(product.into_inner(),"DELETED");

    let payload = serde_json::to_string(&event).unwrap();
    let record = FutureRecord::to(request_topic)
        .key(&correlation_id)
        .payload(&payload);
    println!("sending message {}", payload);
    producer.send(record, Duration::from_secs(0)).await.unwrap();

    let mut stream = consumer.stream();
    'outer: while let Some(message) = match timeout(Duration::from_secs(5), stream.try_next()).await {
        Ok(Ok(msg)) => msg,
        Ok(Err(_)) | Err(_) => return HttpResponse::InternalServerError().body("Failed to get response"),
    } {
        if let Some(payload) = message.payload() {
            consumer.commit_message(&message, rdkafka::consumer::CommitMode::Async).unwrap();
            let response: Product = serde_json::from_slice(payload).unwrap();
            return HttpResponse::Ok().json(response);
        } else {
            continue 'outer;
        }
    }

    HttpResponse::InternalServerError().body("Failed to get response")
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    let product_reply_topic = "product_reply";

    let producer: FutureProducer = ClientConfig::new()
        .set("bootstrap.servers", "localhost:9092")
        .create()
        .expect("Producer creation error");

    let consumer: StreamConsumer = ClientConfig::new()
        .set("group.id", "product_group")
        .set("bootstrap.servers", "localhost:9092")
        .set("enable.auto.commit", "false")
        .set("auto.offset.reset", "earliest")
        .create()
        .expect("Consumer creation error");

    consumer.subscribe(&[product_reply_topic]).expect("Subscription error");

    let producer = Arc::new(producer);
    let consumer = Arc::new(consumer);

    HttpServer::new(move || {
        App::new()
            .app_data(web::Data::new(producer.clone()))
            .app_data(web::Data::new(consumer.clone()))
            .route("/products", web::post().to(create_product))
            .route("/products/{id}", web::put().to(update_product))
            .route("/products/{id}", web::delete().to(delete_product))
    })
    .bind("127.0.0.1:8081")?
    .run()
    .await
}

#[cfg(test)]
mod tests {

    use crate::{create_event, Product};
    use actix_web::http::header::HeaderName;
    use actix_web::http::header::HeaderValue;
    use actix_web::{web, App, HttpRequest, HttpResponse, HttpServer, Responder};
    use async_trait::async_trait;
    use base64::{engine::general_purpose, Engine as _};
    use expectest::prelude::*;
    use maplit::*;
    use pact_models::provider_states::ProviderState;
    use pact_verifier::{
        callback_executors::ProviderStateExecutor, verify_provider_async, FilterInfo,
        NullRequestFilterExecutor, PactSource, ProviderInfo, ProviderTransport,
        VerificationOptions,
    };
    use reqwest::Client;
    use serde_json::json;
    use serde_json::Value;
    use std::{collections::HashMap, env, path::PathBuf, sync::Arc};
    use tokio::sync::oneshot;
    #[derive(Debug)]
    struct DummyProviderStateExecutor;

    #[async_trait]
    impl ProviderStateExecutor for DummyProviderStateExecutor {
        async fn call(
            self: Arc<Self>,
            _interaction_id: Option<String>,
            _provider_state: &ProviderState,
            _setup: bool,
            _client: Option<&Client>,
        ) -> anyhow::Result<HashMap<String, Value>> {
            Ok(hashmap! {})
        }

        fn teardown(self: &Self) -> bool {
            return false;
        }
    }

    async fn start_message_proxy() -> oneshot::Sender<()> {
        async fn handle_request(
            req: HttpRequest,
            body: web::Json<serde_json::Value>,
        ) -> impl Responder {
            println!("Incoming request path: {}", req.path());
            println!("Incoming request method: {}", req.method());
            println!("Incoming request body: {}", body);
            println!("Incoming request description: {}", body["description"]);

            // Incoming request path: /pact-messages
            // Incoming request method: POST
            // Incoming request body: {"description":"a product event update with reply","request":{"contents":{"content":{"event":"UPDATED","id":"some-uuid-1234-5678","name":"Some Product","type":"Product Range","version":"v1"},"contentType":"application/json","encoded":false},"matchingRules":{"body":{"$.event":{"combine":"AND","matchers":[{"match":"regex","regex":"^(CREATED|UPDATED|DELETED)$"}]},"$.id":{"combine":"AND","matchers":[{"match":"type"}]},"$.name":{"combine":"AND","matchers":[{"match":"type"}]},"$.type":{"combine":"AND","matchers":[{"match":"type"}]},"$.version":{"combine":"AND","matchers":[{"match":"type"}]}},"metadata":{}},"metadata":{"kafka_reply_topic":"product_reply","kafka_request_topic":"product_request"}}}

            // TODO - Should we get access to the response contents in the incoming
            // body, to ensure that our provider can handle it?
            // the verification flow here hasn't changed from an async verification
            match body["description"].as_str() {
                Some("a product event update with reply") => {
                    let product = Product {
                        id: Some("some-uuid-1234-5678".to_string()),
                        name: "Some Product".to_string(),
                        r#type: "Product Range".to_string(),
                        version: Some("v1".to_string()),
                    };
                    let event_type = "UPDATED";
                    let product_event = create_event(product, event_type);
                    let mut response = HttpResponse::Ok().json(product_event);
                    let response_metadata = json!({
                      "kafka_reply_topic": "product_reply"
                    });
                    let encoded_metadata =
                        general_purpose::STANDARD.encode(response_metadata.to_string());
                    response.headers_mut().insert(
                        HeaderName::from_static("pact-message-metadata"),
                        HeaderValue::from_str(&encoded_metadata).unwrap(),
                    );
                    response
                }
                _ => HttpResponse::NotFound().finish(),
            }
        }

        let (tx, rx) = oneshot::channel();
        let server = HttpServer::new(|| {
            App::new().route("/pact-messages", web::post().to(handle_request))
            // App::new().default_service(web::route().to(handle_request)) # listen on any route
        })
        .bind("127.0.0.1:8090")
        .expect("Failed to bind server")
        .run();
        let server_handle = server.handle();
        // let _ = server.await;
        tokio::spawn(async move {
            let _ = server.await;
            rx.await.ok();
            server_handle.stop(true).await;
        });

        tx
    }


    #[tokio::test]
    async fn verifies_api_produces_correct_messages_for_consumers() {

        let shutdown_tx = start_message_proxy().await;

        /// Get the path to one of our sample *.json files.
        fn fixture_path(path: &str) -> PathBuf {
            env::current_dir()
                .expect("could not find current working directory")
                .join("..")
                .join("consumer-rust-kafka-sync")
                .join("target")
                .join("pacts")
                .join(path)
                .to_owned()
        }
        let pact_file = fixture_path(
            "pactflow-example-consumer-rust-kafka-sync-pactflow-example-provider-rust-kafka-sync.json",
        );

        #[allow(deprecated)]
        let provider_info = ProviderInfo {
            name: "pactflow-example-provider-rust-kafka-sync".to_string(),
            host: "127.0.0.1".to_string(),
            port: Some(8090),
            transports: vec![ProviderTransport {
                transport: "sync-message".to_string(),
                port: Some(8090),
                path: Some("/pact-messages".to_string()),
                scheme: Some("http".to_string()),
            }],
            ..ProviderInfo::default()
        };

        let pact_source = PactSource::File(pact_file.to_string_lossy().to_string());

        let verification_options: VerificationOptions<NullRequestFilterExecutor> =
            VerificationOptions::default();
        let provider_state_executor = Arc::new(DummyProviderStateExecutor {});

        let result = verify_provider_async(
            provider_info,
            vec![pact_source],
            FilterInfo::None,
            vec![],
            &verification_options,
            None,
            &provider_state_executor,
            None,
        )
        .await;

        // shutdown our message proxy
        shutdown_tx
        .send(())
        .expect("Failed to send shutdown signal");

        // check the verification results
        match result {
            Ok(res) => {
                if res.result {
                    expect!(res.result).to(be_equal_to(true));
                } else {
                    panic!("Pact verification failed");
                }
            },
            Err(error) => panic!("failed to get pact verification execution result {}",error),
        }
    }
}
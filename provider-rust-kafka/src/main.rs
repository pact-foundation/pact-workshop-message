use actix_web::{web, App, HttpResponse, HttpServer, Responder};
use rdkafka::config::ClientConfig;
use rdkafka::producer::{FutureProducer, FutureRecord};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::Mutex;
#[derive(Serialize, Deserialize, Clone)]
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

pub struct ProductEventService {
    producer: Arc<Mutex<FutureProducer>>,
    topic: String,
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

impl ProductEventService {
    async fn new(broker: &str, topic: &str) -> Self {
        let producer: FutureProducer = ClientConfig::new()
            .set("bootstrap.servers", broker)
            .create()
            .expect("Producer creation error");

        ProductEventService {
            producer: Arc::new(Mutex::new(producer)),
            topic: topic.to_string(),
        }
    }

    // pub fn create_event(&self, product: Product, event_type: &str) -> ProductEvent {
    //     let version = increment_version(product.version);
    //     ProductEvent {
    //         id: product
    //             .id
    //             .unwrap_or_else(|| uuid::Uuid::new_v4().to_string()),
    //         name: product.name,
    //         r#type: product.r#type,
    //         event: event_type.to_string(),
    //         version,
    //     }
    // }

    async fn publish(&self, event: ProductEvent) {
        let payload = serde_json::to_string(&event).unwrap();
        let record = FutureRecord::<String, String>::to(&self.topic).payload(&payload);
        let producer = self.producer.lock().await;
        producer
            .send(record, rdkafka::util::Timeout::Never)
            .await
            .unwrap();
    }

    async fn create(&self, product: Product) {
        let event = create_event(product, "CREATED");
        self.publish(event).await;
    }

    async fn update(&self, product: Product) {
        let event = create_event(product, "UPDATED");
        self.publish(event).await;
    }

    async fn delete(&self, product: Product) {
        let event = create_event(product, "DELETED");
        self.publish(event).await;
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
    service: web::Data<Arc<ProductEventService>>,
    product: web::Json<Product>,
) -> impl Responder {
    service.create(product.into_inner()).await;
    HttpResponse::Created().finish()
}

async fn update_product(
    service: web::Data<Arc<ProductEventService>>,
    product: web::Json<Product>,
) -> impl Responder {
    service.update(product.into_inner()).await;
    HttpResponse::Ok().finish()
}

async fn delete_product(
    service: web::Data<Arc<ProductEventService>>,
    product: web::Json<Product>,
) -> impl Responder {
    service.delete(product.into_inner()).await;
    HttpResponse::Ok().finish()
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    let broker = "localhost:9092";
    let topic = "products";
    let service = Arc::new(ProductEventService::new(broker, topic).await);

    HttpServer::new(move || {
        App::new()
            .app_data(web::Data::new(service.clone()))
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
            _client: std::option::Option<&reqwest::Client>,
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
            println!("Incoming request path: {}", req.method());
            println!("Incoming request body: {}", body);
            println!("Incoming request body: {}", body["description"]);

            match body["description"].as_str() {
                Some("a product event update") => {
                    let product = Product {
                        id: Some("some-uuid-1234-5678".to_string()),
                        name: "Some Product".to_string(),
                        r#type: "Product Range".to_string(),
                        version: Some("v1".to_string()),
                    };
                    let event_type = "UPDATED";
                    let product_event = create_event(product, event_type);
                    let mut response = HttpResponse::Ok().json(product_event);
                    let metadata = json!({
                      "kafka_topic": "products"
                    });
                    let encoded_metadata =
                        general_purpose::STANDARD.encode(metadata.to_string());
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
                .join("consumer-rust-kafka")
                .join("target")
                .join("pacts")
                .join(path)
                .to_owned()
        }
        let pact_file = fixture_path(
            "pactflow-example-consumer-rust-kafka-pactflow-example-provider-rust-kafka.json",
        );

        #[allow(deprecated)]
        let provider_info = ProviderInfo {
            name: "pactflow-example-provider-rust-kafka".to_string(),
            host: "127.0.0.1".to_string(),
            port: Some(8090),
            transports: vec![ProviderTransport {
                transport: "async-message".to_string(),
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

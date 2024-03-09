use aws_lambda_events::event::s3::S3Event;use lambda_runtime::{run, service_fn, LambdaEvent};
use anyhow::{Result, Error};
use aws_sdk_s3::Client;
use aws_config::meta::region::RegionProviderChain;
use std::{path::PathBuf};
use tokio::fs::File;
use tokio::fs::File as TokioFile;
use tokio::io::{AsyncBufReadExt, BufReader};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug)]
struct MyData {
    adult: bool,
    id: i16,
    original_title: String,
    popularity: f32,
    video: bool,
}
async fn function_handler(event: LambdaEvent<S3Event>) -> Result<()> {
    println!("Received event: {:?}", event);
    let region_provider = RegionProviderChain::default_provider().or_else("us-east-1");
    let config = aws_config::from_env().region(region_provider).load().await;
    let client = Client::new(&config);
    
    let sqs_client = aws_sdk_sqs::Client::new(&config);

    // Extract some useful information from the request
    let bucket = event.payload.records[0].s3.bucket.name.clone().unwrap();
    let key = event.payload.records[0].s3.object.key.clone().unwrap();

    let path = download_file(&client, &bucket, &key).await?;
    println!("Downloaded file to {:?}", path);

    process_ndjson(&path, sqs_client).await.expect("failed to process NDJSON file");

    Ok(())
}

async fn process_ndjson(path: &PathBuf, client: aws_sdk_sqs::Client) -> Result<()> {
    // Asynchronously open the NDJSON file.
    let file = TokioFile::open(path).await.expect("failed to open file");
    let reader = BufReader::new(file);
    let mut lines = reader.lines();
    let queue_url = "https://sqs.us-east-1.amazonaws.com/831939475229/bacon-queue";

    // Process each line.
    while let Some(line) = lines.next_line().await.expect("failed to read line") {
        // Parse the JSON line into the Rust struct.
        let data: MyData = serde_json::from_str(&line)?;
        println!("Parsed data: {:?}", data);
        client.send_message()
            .queue_url(queue_url)
            .message_body(line)
            .send()
            .await?;
    }

    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        // disable printing the name of the module in every log line.
        .with_target(false)
        // disabling time is handy because CloudWatch will add the ingestion time.
        .without_time()
        .init();

    run(service_fn(function_handler)).await.expect("TODO: panic message");
    Ok(())
}

async fn download_file(client: &Client, bucket: &str, key: &str) -> Result<PathBuf, Error> {
    let path = PathBuf::from("/tmp").join(key);
    let mut file = File::create(&path).await?;
    let mut stream = client
        .get_object()
        .bucket(bucket)
        .key(key)
        .send()
        .await?
        .body
        .into_async_read();
    tokio::io::copy(&mut stream, &mut file).await?;
    Ok(path)
}

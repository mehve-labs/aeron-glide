//! Request/response over response channels (`control-mode=response`), with an
//! embedded media driver: the server answers each client on a channel the
//! client controls, without knowing the client's address up front.
//!
//! cargo run --example response_channel

use aeron_glide::{
    AeronClient, ChannelBuilder, Context, ControlMode, ImageEvent, MediaDriver, Publication,
    Result, ThreadingMode,
};
use std::sync::mpsc;
use std::time::{Duration, Instant};

const REQUEST_ENDPOINT: &str = "localhost:20121";
const RESPONSE_CONTROL: &str = "localhost:20122";
const REQUEST_STREAM: i32 = 1001;
const RESPONSE_STREAM: i32 = 1002;

fn main() -> Result<()> {
    let dir = std::env::temp_dir().join(format!("aeron-glide-response-{}", std::process::id()));
    let dir = dir.to_string_lossy().into_owned();
    let _driver = MediaDriver::builder()
        .dir(&dir)
        .dir_delete_on_start(true)
        .dir_delete_on_shutdown(true)
        .threading_mode(ThreadingMode::Shared)
        .start()?;
    let server = AeronClient::connect(Context::new().aeron_dir(&dir))?;
    let client = AeronClient::connect(Context::new().aeron_dir(&dir))?;

    // Server: a plain subscription for requests. Each client shows up as an image.
    let (images_tx, images) = mpsc::channel::<ImageEvent>();
    let mut requests = server.add_subscription_with_image_handlers(
        &ChannelBuilder::udp().endpoint(REQUEST_ENDPOINT).build()?,
        REQUEST_STREAM,
        move |image| {
            let _ = images_tx.send(image.clone());
        },
        |_| {},
    )?;

    // Client: a response subscription on its control address, then a request
    // publication tagged with the response subscription's registration ID.
    let mut responses = client.add_subscription(
        &ChannelBuilder::udp()
            .control_mode(ControlMode::Response)
            .control_endpoint(RESPONSE_CONTROL)
            .build()?,
        RESPONSE_STREAM,
    )?;
    let request_publication = client.add_publication(
        &ChannelBuilder::udp()
            .endpoint(REQUEST_ENDPOINT)
            .response_correlation_id(responses.registration_id())
            .build()?,
        REQUEST_STREAM,
    )?;

    // Server: a response publication for the client's request image, tagged with
    // the image's correlation ID.
    let image = images
        .recv_timeout(Duration::from_secs(10))
        .expect("the client's request image");
    let response_publication = server.add_publication(
        &ChannelBuilder::udp()
            .control_mode(ControlMode::Response)
            .control_endpoint(RESPONSE_CONTROL)
            .response_correlation_id(image.correlation_id)
            .build()?,
        RESPONSE_STREAM,
    )?;

    send(&request_publication, b"ping")?;
    let mut request = Vec::new();
    until(|| {
        requests.poll(1, |data, _| request = data.to_vec())?;
        Ok(!request.is_empty())
    })?;
    println!("server received {:?}", String::from_utf8_lossy(&request));

    send(&response_publication, b"pong")?;
    let mut response = Vec::new();
    until(|| {
        responses.poll(1, |data, _| response = data.to_vec())?;
        Ok(!response.is_empty())
    })?;
    println!("client received {:?}", String::from_utf8_lossy(&response));
    Ok(())
}

/// Offer once the publication is connected, retrying back pressure.
fn send(publication: &Publication, message: &[u8]) -> Result<()> {
    until(|| match publication.offer(message) {
        Ok(_) => Ok(true),
        Err(e) if e.is_retryable() => Ok(false),
        Err(e) => panic!("offer failed: {e}"),
    })
}

fn until(mut done: impl FnMut() -> Result<bool>) -> Result<()> {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !done()? {
        assert!(Instant::now() < deadline, "timed out");
        std::thread::sleep(Duration::from_millis(1));
    }
    Ok(())
}

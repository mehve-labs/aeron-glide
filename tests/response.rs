#![cfg(feature = "driver")]
//! Response channels (`control-mode=response`): a server answers each client
//! on a channel the client controls.

mod common;

use aeron_glide::{ChannelBuilder, ChannelUri, ControlMode, ImageEvent};
use common::{TestDriver, free_udp_port, offer, poll_n, wait_until};
use std::sync::{Arc, Mutex};

#[test]
fn request_response_over_response_channels() {
    let driver = TestDriver::start();
    let server = driver.client();
    let request_endpoint = format!("localhost:{}", free_udp_port());

    // Server: a plain request subscription; remember each client's image.
    let images = Arc::new(Mutex::new(Vec::<ImageEvent>::new()));
    let sink = images.clone();
    let mut requests = server
        .add_subscription_with_image_handlers(
            &ChannelBuilder::udp()
                .endpoint(&request_endpoint)
                .build()
                .unwrap(),
            1,
            move |image| sink.lock().unwrap().push(image.clone()),
            |_| {},
        )
        .unwrap();

    // Two clients, each with its own response subscription.
    let mut clients = Vec::new();
    for _ in 0..2 {
        let client = driver.client();
        let response_control = format!("localhost:{}", free_udp_port());
        let response_channel = ChannelBuilder::udp()
            .control_mode(ControlMode::Response)
            .control_endpoint(&response_control)
            .build()
            .unwrap();
        assert!(
            ChannelUri::parse(&response_channel)
                .unwrap()
                .has_control_mode_response()
        );
        let responses = client.add_subscription(&response_channel, 2).unwrap();
        let request_publication = client
            .add_publication(
                &ChannelBuilder::udp()
                    .endpoint(&request_endpoint)
                    .response_correlation_id(responses.registration_id())
                    .build()
                    .unwrap(),
                1,
            )
            .unwrap();
        clients.push((client, response_control, responses, request_publication));
    }

    wait_until("both request images", || images.lock().unwrap().len() == 2);
    for (_, _, _, request_publication) in &clients {
        wait_until("the request publication to connect", || {
            request_publication.is_connected()
        });
    }

    // Server: one response publication per request image, tagged with the
    // image's correlation ID and sent to the client's response control address.
    let images = images.lock().unwrap().clone();
    let mut responders = Vec::new();
    for (i, (_, response_control, _, request_publication)) in clients.iter().enumerate() {
        let image = images
            .iter()
            .find(|image| image.session_id == request_publication.session_id())
            .expect("the client's request image");
        let response_publication = server
            .add_publication(
                &ChannelBuilder::udp()
                    .control_mode(ControlMode::Response)
                    .control_endpoint(response_control)
                    .response_correlation_id(image.correlation_id)
                    .build()
                    .unwrap(),
                2,
            )
            .unwrap();
        responders.push((i, response_publication));
    }

    // Each client sends a request; the server answers on that client's response
    // publication, and only that client receives the answer.
    for (i, (_, _, _, request_publication)) in clients.iter().enumerate() {
        offer(request_publication, format!("request {i}").as_bytes());
    }
    let mut received = Vec::new();
    poll_n(&mut requests, 2, |data| {
        received.push(String::from_utf8_lossy(data).into_owned())
    });
    received.sort();
    assert_eq!(received, ["request 0", "request 1"]);

    for (i, response_publication) in &responders {
        wait_until("the response publication to connect", || {
            response_publication.is_connected()
        });
        offer(response_publication, format!("response {i}").as_bytes());
    }
    for (i, (_, _, responses, _)) in clients.iter_mut().enumerate() {
        let mut got = Vec::new();
        poll_n(responses, 1, |data| {
            got.push(String::from_utf8_lossy(data).into_owned())
        });
        assert_eq!(got, [format!("response {i}")]);
    }
    // Nothing else arrives.
    std::thread::sleep(std::time::Duration::from_millis(100));
    for (_, _, responses, _) in clients.iter_mut() {
        assert_eq!(responses.poll(10, |_, _| {}).unwrap(), 0);
    }
}

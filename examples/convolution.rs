use std::fs::File;
use std::{thread, time};

use web_audio_api::context::{AudioContext, BaseAudioContext};
use web_audio_api::node::{AudioNode, AudioScheduledSourceNode, ConvolverNode, ConvolverOptions};

fn main() {
    // create an `AudioContext` and load a sound file
    let context = AudioContext::default();
    let file = File::open("samples/vocals-dry.wav").unwrap();
    let audio_buffer = context.decode_audio_data_sync(file).unwrap();

    let impulse_file1 = File::open("samples/small-room-response.wav").unwrap();
    let impulse_buffer1 = context.decode_audio_data_sync(impulse_file1).unwrap();

    let impulse_file2 = File::open("samples/parking-garage-response.wav").unwrap();
    let impulse_buffer2 = context.decode_audio_data_sync(impulse_file2).unwrap();

    let src = context.create_buffer_source();
    src.set_buffer(audio_buffer);

    let mut convolve = ConvolverNode::new(&context, ConvolverOptions::default());

    src.connect(&convolve);
    convolve.connect(&context.destination());

    src.start();

    println!("Dry");
    thread::sleep(time::Duration::from_millis(4_000));

    println!("Small room");
    convolve.set_buffer(Some(impulse_buffer1));
    thread::sleep(time::Duration::from_millis(4_000));

    println!("Parking garage");
    convolve.set_buffer(Some(impulse_buffer2));
    thread::sleep(time::Duration::from_millis(9_000));
}

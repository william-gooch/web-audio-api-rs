use std::fs::File;
use web_audio_api::context::{AsBaseAudioContext, AudioContext};
use web_audio_api::node::AudioNode;

// experimental API
// use web_audio_api::audio_buffer::decode_audio_data;

fn main() {
    let context = AudioContext::new(None);
    // @note - `resume` does not seem to be needed

    // load and decode buffer
    let file = File::open("sample.wav").unwrap();
    let audio_buffer = context.decode_audio_data(file);

    // @fixme - if only one node in the graph it is never removed even when returning
    // false, se we put this dummy node in the graph so that other ones are properly
    // removed
    let mut src = context.create_buffer_source();
    src.set_buffer(&audio_buffer);
    src.connect(&context.destination());

    {
        println!("++ play until end");
        let mut src = context.create_buffer_source();
        src.set_buffer(&audio_buffer);
        src.connect(&context.destination());
        src.start_at(context.current_time());
    }

    std::thread::sleep(std::time::Duration::from_millis(3500));

    {
        println!("++ play / stop 1sec");
        let mut src = context.create_buffer_source();
        src.set_buffer(&audio_buffer);
        src.connect(&context.destination());
        src.start_at(context.current_time());
        src.stop_at(context.current_time() + 1.);
    }

    std::thread::sleep(std::time::Duration::from_millis(1500));

    {
        println!("++ play / stop 1sec with offset");
        let mut src = context.create_buffer_source();
        src.set_buffer(&audio_buffer);
        src.connect(&context.destination());
        src.start_at_with_offset(context.current_time(), 1.);
        src.stop_at(context.current_time() + 1.);
    }

    std::thread::sleep(std::time::Duration::from_millis(1500));

    {
        println!("++ play 1sec with offset and duration");
        let mut src = context.create_buffer_source();
        src.set_buffer(&audio_buffer);
        src.connect(&context.destination());
        src.start_at_with_offset_and_duration(context.current_time(), 1., 1.);
    }

    std::thread::sleep(std::time::Duration::from_millis(1500));

    {
        println!("++ play backward from offset 1.");
        let mut src = context.create_buffer_source();
        src.set_buffer(&audio_buffer);
        src.connect(&context.destination());
        src.playback_rate().set_value(-1.);
        src.start_at_with_offset(context.current_time(), 1.);
    }

    std::thread::sleep(std::time::Duration::from_millis(1500));

    {
        println!("++ play backward full buffer");
        let mut src = context.create_buffer_source();
        src.set_buffer(&audio_buffer);
        src.connect(&context.destination());
        src.playback_rate().set_value(-1.);
        src.start_at_with_offset(context.current_time(), audio_buffer.duration());
    }

    std::thread::sleep(std::time::Duration::from_millis(3500));

    {
        println!("++ simple loop (x2)");
        let mut src = context.create_buffer_source();
        src.set_buffer(&audio_buffer);
        src.connect(&context.destination());
        src.set_loop(true);
        src.start_at(context.current_time());
        src.stop_at(context.current_time() + audio_buffer.duration() * 2.);
    }

    std::thread::sleep(std::time::Duration::from_millis(7000));

    {
        println!("++ loop between 1 and 2 starting from 0");
        let mut src = context.create_buffer_source();
        src.set_buffer(&audio_buffer);
        src.connect(&context.destination());
        src.set_loop(true);
        src.set_loop_start(1.);
        src.set_loop_end(2.);
        src.start_at(context.current_time());

        std::thread::sleep(std::time::Duration::from_millis(4500));
        src.set_loop(false);
    }

    std::thread::sleep(std::time::Duration::from_millis(2500));

    {
        println!("++ loop backward between 1 and 2 starting from end");
        let mut src = context.create_buffer_source();
        src.set_buffer(&audio_buffer);
        src.connect(&context.destination());
        src.playback_rate().set_value(-1.);
        src.set_loop(true);
        src.set_loop_start(1.);
        src.set_loop_end(2.);
        src.start_at_with_offset(context.current_time(), audio_buffer.duration());

        std::thread::sleep(std::time::Duration::from_millis(4500));
        src.set_loop(false);
    }

    std::thread::sleep(std::time::Duration::from_millis(2500));

    println!("++ end of examples");

    for i in 0..9 {
        let offset = i as f64 / 2.;

        let gain = if i % 4 == 0 { 1. } else { 0.2 };
        let env = context.create_gain();
        env.gain().set_value(gain);
        env.connect(&context.destination());

        let mut src = context.create_buffer_source();
        src.set_buffer(&audio_buffer);
        src.connect(&env);
        src.start_at(context.current_time() + offset);
    }

    std::thread::sleep(std::time::Duration::from_millis(8000));

    // some stress test
    // let num_sources_by_sec = 100; // one source per 10ms
    // // 100 is ok
    // // 200 starts to click
    // // 1000 is really not ok
    // // ...let's agree 10ms is ok for descent granular synthesis

    // for i in 0..num_sources_by_sec {
    //   // let offset = i as f64 / 1000.; // 10ms - look ok in --release
    //   let offset = i as f64 / num_sources_by_sec as f64;

    //   // this starts to look like home :)
    //   let gain = if i % 4 == 0 { 1. } else { 0.3 };
    //   let env = context.create_gain();
    //   env.gain().set_value(gain);
    //   env.connect(&context.destination());

    //   let mut src = context.create_buffer_source();
    //   src.set_buffer(&audio_buffer);
    //   src.connect(&env);
    //   src.start_at(context.current_time() + offset);
    // }

    // std::thread::sleep(std::time::Duration::from_secs(8));
}

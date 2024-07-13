use std::{thread, time::Duration};
use std::io::stdin;
use anyhow::{Context, Result};
mod player;


fn main() -> Result<()> {
    let audio_file = "./test_transmission.mp3";
    
    loop {
        let player = player::Player::for_devices("/dev/ttyUSB0".to_string(), "front:CARD=Device,DEV=0".to_string())
            .context("Failed to initialize player")?;

        player.queue_audio(audio_file.to_string())?;

        let mut is_paused = true;
        let mut count = 0;
        loop {
            let mut s = String::new();
            println!("Press ENTER to play/pause audio");
            stdin().read_line(&mut s).context("Failed to read user input")?;

            if is_paused {
                player.play()?;
            } else {
                player.pause()?;
            }
            is_paused = !is_paused;
            count += 1;
            if count > 1 {
                break
            }
        }

        println!("Starting all over again in 2 seconds...");
        thread::sleep(Duration::from_secs(2));
        println!("Let's go!");
    }
}

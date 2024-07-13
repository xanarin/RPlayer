use std::{thread, time::Duration, fs::File};
use std::io::BufReader;
use rodio::{Decoder, DeviceTrait, OutputStream, Sink};
use rodio::cpal;
use rodio::cpal::traits::HostTrait;
use anyhow::{anyhow, Context, Result};
use nix::{fcntl, ioctl_read_bad};

const IOCTL_TIOCMGET:i32 = 0x5415;
const IOCTL_TIOCMSET:i32 = 0x5418;

const TIOCM_RTS_FLAG:i32 = 0x004;

pub struct Player {
    tty_fd: i32,

    sink: Sink,
    // 'stream' must have the same lifetime as 'sink', or audio playback will be halted when 'stream' is dropped
    #[allow(dead_code)]
    stream: OutputStream,
}

impl Player {
    // Digirig always appears with CARD=Device in the name, and that appears to be unique to
    // usb-attached sound devices:
    // # Device: sysdefault:CARD=Device
    // # Device: front:CARD=Device,DEV=0
    // # Device: surround40:CARD=Device,DEV=0
    // # Device: iec958:CARD=Device,DEV=0
    //
    //
    pub fn for_devices(tty_path: String, audio_device_name: String) -> Result<Player> {
        // Set up audio output
        let host = cpal::default_host();
        let output_devs = host
            .output_devices()
            .with_context(|| "Failed to enumerate output devices")?;

        let mut output_dev:Option<rodio::Device> = None;
        // List output devices and find our target device
        for dev in output_devs {
            if let Ok(name) = dev.name() {
                if name == audio_device_name {
                    output_dev = dev.into();
                }
            }
        };

        // We assert that the Option is not None with .context()
        let output_dev = output_dev.context(format!("Failed to find audio device '{}'", audio_device_name))?;

        // If 'stream' is dropped, the stream_handle and sink are useless. See this note from the
        // rodio documentation:
        //   > If [the OutputStream] is dropped playback will end [and] attached OutputStreamHandles will no longer work.
        let (stream, stream_handle) = OutputStream::try_from_device(&output_dev).unwrap();
        let sink = Sink::try_new(&stream_handle).context("Failed to create Sink from output device")?;

        // Set up TTY device
        let tty_fd =  fcntl::open(tty_path.as_str(), fcntl::OFlag::O_RDWR,
                                    nix::sys::stat::Mode::S_IRWXU)
            .context("Failed to open TTY device")?;
        // Ensure that RTS is NOT asserted so we don't hold open the RF link on startup
        let player = Player{tty_fd, sink, stream};
        if player.rts_is_enabled()? {
            player.toggle_rts()?
        }

        Ok(player)
    }

    pub fn queue_audio(self: &Player, audiofile_path: String) -> Result<()> {
        let file = BufReader::new(File::open(&audiofile_path).context("Failed to open audio file")?);
        let source = Decoder::new(file).context("Failed to create decoder for audio file")?;

        println!("Playing audio file {}", audiofile_path);
        self.sink.append(source);
        self.sink.pause();

        Ok(())
    }

    pub fn play(self: &Player) -> Result<()> {
        if self.rts_is_enabled()? || !self.sink.is_paused() {
            return Err(anyhow!("Cannot play because streaming is already in progress"));
        }

        self.toggle_rts()?;
        // Sleep for a short period so that audio doesn't get cut off
        thread::sleep(Duration::from_millis(250));
        self.sink.play();

        Ok(())
    }

    pub fn pause(self: &Player) -> Result<()> {
        if !self.rts_is_enabled()? || self.sink.is_paused() {
            return Err(anyhow!("Cannot play because streaming is already paused"));
        }

        self.sink.pause();
        // Sleep for a short period so that audio doesn't get cut off
        thread::sleep(Duration::from_millis(250));
        self.toggle_rts()?;

        Ok(())
    }

    // We need the *_bad variants here because these are "old"-style syscalls
    ioctl_read_bad!(tiocmget, IOCTL_TIOCMGET, i32);
    ioctl_read_bad!(tiocmset, IOCTL_TIOCMSET, i32);

    pub fn rts_is_enabled(self: &Player) -> Result<bool> {
        let mut control_bits:i32 = 0;

        unsafe { Player::tiocmget(self.tty_fd, &mut control_bits) }
            .map_err(|e| anyhow!("Failed to get tty parameters: {}", e))?;
            
        Ok((control_bits & TIOCM_RTS_FLAG) != 0)
    }

    pub fn toggle_rts(self: &Player) -> Result<()> {
        let mut control_bits:i32 = 0;

        unsafe { Player::tiocmget(self.tty_fd, &mut control_bits) }
            .map_err(|e| anyhow!("Failed to get tty parameters: {}", e))?;

        control_bits ^= TIOCM_RTS_FLAG;

        unsafe { Player::tiocmset(self.tty_fd, &mut control_bits) }
            .map_err(|e| anyhow!("Failed to set tty parameters: {}", e))?;
            
        Ok(())
    }
}

impl Drop for Player {
    fn drop(&mut self) {
        // Because we have a raw FD from nix::fcntl, we need to explicitly close(2) it here in
        // order to not leak the FD. This is basically an assertion so panicking on failure is
        // acceptable.
        nix::unistd::close(self.tty_fd).expect("Failed to close fd");
    }
}


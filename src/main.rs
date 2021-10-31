extern crate image;
extern crate rusticnes_core;
extern crate rusticnes_ui_common;

use rusticnes_core::nes::NesState;
use rusticnes_core::palettes::NTSC_PAL;
use rusticnes_core::cartridge::mapper_from_file;

use rusticnes_ui_common::application::RuntimeState as RusticNesRuntimeState;
use rusticnes_ui_common::events;

use std::env;
use std::fs::File;
use std::str;

use std::io::Read;
use std::io::Write;
use std::io::BufReader;
use std::io::BufRead;

pub struct CliRuntimeState {
  pub core: RusticNesRuntimeState,
  pub game_file: Option<File>,
  pub audio_file: Option<File>,
}

impl CliRuntimeState {
  pub fn new() -> CliRuntimeState {
    return CliRuntimeState{
      core: RusticNesRuntimeState::new(),
      game_file: None,
      audio_file: None,
    }
  }
}

pub fn dispatch_event(state: &mut CliRuntimeState, event: events::Event) {
  let mut responses: Vec<events::Event> = Vec::new();
  // Process application events here, passing in a reference to core state
  // (there aren't any yet!)

  // Now process core state, passing in a reference to itself
  responses.extend(state.core.handle_event(event.clone()));

  // Finally, recursively dispatch any responses we got to this event, bubbling those up the chain
  for response in responses {
    dispatch_event(state, response);
  }
}

fn load_cartridge(nes: &mut NesState, cartridge_path: &str) {
  // Read in the ROM file and attempt to create a new NesState:
  let file = File::open(cartridge_path);
  match file {
    Err(why) => {
      panic!("Couldn't open {}: {}", cartridge_path, why);
    },
    Ok(_) => (),
  };

  // Read the whole thing
  let mut cartridge = Vec::new();
  match file.unwrap().read_to_end(&mut cartridge) {
    Err(why) => {
      panic!("Couldn't read from {}: {}", cartridge_path, why);
    },
    Ok(_) => {
      println!("Loading {}...", cartridge_path);
      let maybe_mapper = mapper_from_file(&cartridge);
      match maybe_mapper {
            Ok(mapper) => {
              *nes = NesState::new(mapper);
              nes.power_on();
            },
        Err(why) => {
          panic!("{}", why);
        }
      }
    },
  };
}

// Note: Later we should use the ui-common library, and dump panels instead of just the game screen. That
// will be very flexible and useful.
fn dump_frame(state: &mut CliRuntimeState) {
  match &mut state.game_file {
    Some(file) => {
      let mut rgba_pixels: [u8; 3 * 256 * 240] = [0; 3 * 256 * 240]; 
      for x in 0 .. 256 {
        for y in 0 .. 240 {
          let palette_index = ((state.core.nes.ppu.screen[y * 256 + x]) as usize) * 3;
          let pixel_index = (256 * y + x) * 3;
          rgba_pixels[pixel_index + 0] = NTSC_PAL[palette_index + 0];
          rgba_pixels[pixel_index + 1] = NTSC_PAL[palette_index + 1];
          rgba_pixels[pixel_index + 2] = NTSC_PAL[palette_index + 2];
        }
      }
      let _ = file.write_all(&rgba_pixels);
    },
    None => {}
  }
}

fn dump_audio(state: &mut CliRuntimeState) {
  match &mut state.audio_file {
    Some(file) => {
      if state.core.nes.apu.buffer_full {
        let buffer_size = state.core.nes.apu.output_buffer.len();
        for i in 0 .. buffer_size {
          let _ = file.write_all(&state.core.nes.apu.output_buffer[i].to_be_bytes());
        }
        state.core.nes.apu.buffer_full = false;
      }
    },
    None => {}
  }
}


fn run(state: &mut CliRuntimeState, frames: u64) {
  for _ in 0 .. frames {
    //state.core.nes.run_until_vblank();
    dispatch_event(state, events::Event::NesRunFrame);
    dump_frame(state);
    dump_audio(state);
  }
}

fn reset(nes: &mut NesState) {
  nes.reset();
}

fn tap(state: &mut CliRuntimeState, button: &str, frames: u64) {
  let button_index: u8 = match button {
    "a" => 0,
    "b" => 1,
    "select" => 2,
    "start" => 3,
    "up" => 4,
    "down" => 5,
    "left" => 6,
    "right" => 7,
    _ => panic!("Invalid button to tap: {}", button)
  };
  // TODO: change button state using application events?
  state.core.nes.p1_input |= 0x1 << button_index;
  run(state, frames);
  state.core.nes.p1_input ^= 0x1 << button_index;
}

fn save_screenshot(nes: &NesState, output_path: &str) {
  let mut img = image::ImageBuffer::new(256, 240);
  for x in 0 .. 256 {
    for y in 0 .. 240 {
      let palette_index = ((nes.ppu.screen[y * 256 + x]) as usize) * 3;
      img.put_pixel(x as u32, y as u32, image::Rgba([
        NTSC_PAL[palette_index + 0],
        NTSC_PAL[palette_index + 1],
        NTSC_PAL[palette_index + 2],
        255 as u8]));
    }
  }

  image::ImageRgba8(img).save(output_path).unwrap();

  println!("Saved screenshot to {}", output_path);
}

fn save_blargg(nes: &mut NesState, output_filename: &str) {
  if nes.mapper.has_sram() {
    let sram = nes.mapper.get_sram();

    let test_status = sram[0];
    let magic_0 = sram[1];
    let magic_1 = sram[2];
    let magic_2 = sram[3];

    if magic_0 == 0xDE && magic_1 == 0xB0 && magic_2 == 0x61 {
      let test_status_string = match test_status {
        0x80 => format!("Running"),
        0x81 => format!("Needs RESET"),
        _ => format!("0x{:02X}", test_status),
      };


      // Starting at 0x6004, read all of NES memory up to the next null terminator (or 0x8000) as ASCII
      let begin = 4;
      // Locate the next null terminator
      let mut end = 4;
      while sram[end] != 0  && end < sram.len() {
        end+=1;
      }

      let test_text = str::from_utf8(&sram[begin .. end]).unwrap();
      let output = format!("Test Status: {}\n\n{}", test_status_string, test_text);

      // Output!
      let ref mut file = File::create(output_filename).unwrap();
      let _ = file.write_all(output.as_ref());
      println!("Saved blargg data to {}", output_filename);
    } else {
      let ref mut file = File::create(output_filename).unwrap();
      let _ = file.write_all(format!("Invalid blargg magic header, found 0x{:02X} 0x{:02X} 0x{:02X} instead.", magic_0, magic_1, magic_2).as_ref());
    }    
  } else {
    panic!("Cannot output blargg data, ROM has no SRAM!");
  }
}

fn command_file(state: &mut CliRuntimeState, command_path: &str) {
  let file = File::open(command_path);
  match file {
    Err(why) => {
      panic!("Couldn't open {}: {}", command_path, why);
    },
    Ok(_) => (),
  };

  let unwrapped_file = file.unwrap();
  let file_reader = BufReader::new(&unwrapped_file);
  for l in file_reader.lines() {
    let line = l.unwrap();
    let command_list = line.split(" ").map(|s| s.to_string()).collect();
    process_command_list(state, command_list);
  }
}

fn process_command_list(state: &mut CliRuntimeState, mut command_list: Vec<String>) {
  while command_list.len() > 0 {
    let command = command_list.remove(0);
    match command.as_ref() {
      "cart" | "cartridge" | "rom" => {
        // TODO: implement this with the standard event instead
        let cartridge_path = command_list.remove(0);
        load_cartridge(&mut state.core.nes, cartridge_path.as_ref());
      },
      "run" | "frames" => {
        let frames: u64 = command_list.remove(0).parse().unwrap();
        run(state, frames);
      },
      "reset" => {
        // TODO: implement this with the standard event instead
        reset(&mut state.core.nes);
      }
      "tap" => {
        let button = command_list.remove(0);
        let frames: u64 = command_list.remove(0).parse().unwrap();
        tap(state, button.as_ref(), frames);
      }
      "screenshot" => {
        let cartridge_path = command_list.remove(0);
        save_screenshot(&mut state.core.nes, cartridge_path.as_ref());
      },
      "blargg" => {
        let output_path = command_list.remove(0);
        save_blargg(&mut state.core.nes, output_path.as_ref());
      },
      "fromfile" => {
        let command_file_path = command_list.remove(0);
        command_file(state, command_file_path.as_ref());
      },
      "video" => {
        let panel = command_list.remove(0);
        let output_path = command_list.remove(0);
        match panel.as_str() {
          "game" => {
            match File::create(&output_path) {
              Err(why) => {
                panic!("Couldn't open {}: {}", output_path, why);
              },
              Ok(file) => {
                state.game_file = Some(file);
              }
            }
          },
          _ => {
            println!("Unrecognized panel name {}, ignoring", panel);
          }
        }
      },
      "audio" => {
        let output_path = command_list.remove(0);
        match File::create(&output_path) {
          Err(why) => {
            panic!("Couldn't open {}: {}", output_path, why);
          },
          Ok(file) => {
            state.audio_file = Some(file);
          }
        }
      }
      "#" => {
        // A comment! Everything on this line is discarded
        return;
      }
      "" => {
        // Do nothing. This allows blank lines to exist.
      }
      _ => {
        panic!("Unrecognized command: {}\n\nChaos reigns within\nReflect, repent, and retry\nOrder shall return\n", command);
      }
    }
  }    
}

fn main() {
	let mut args: Vec<_> = env::args().collect();
  if args.len() < 2 {
    panic!("Usage: rusticnes-cli <commands>");
  }

  let mut state = CliRuntimeState::new();

  // Pop off the name of the program
  let _ = args.remove(0);

  process_command_list(&mut state, args);
}

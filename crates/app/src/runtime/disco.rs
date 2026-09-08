use super::*;

pub(super) const DISCO_PALETTE_BGRA: [[u8; 3]; 6] = [
    [190, 28, 255],
    [255, 176, 24],
    [82, 245, 36],
    [34, 224, 255],
    [255, 70, 110],
    [214, 38, 255],
];

pub(super) struct DiscoEffect {
    pub(super) x_band: Vec<u8>,
    pub(super) x_boost: Vec<u8>,
    pub(super) y_boost: Vec<u8>,
    pub(super) frame_pool: FrameBufferPool,
}

impl DiscoEffect {
    pub(super) fn new(size: Size) -> Self {
        Self {
            x_band: vec![0; size.width as usize],
            x_boost: vec![0; size.width as usize],
            y_boost: vec![0; size.height as usize],
            frame_pool: FrameBufferPool::new(
                size.width as usize * size.height as usize * 4,
                CAPTURE_FRAME_POOL_CAPACITY,
            ),
        }
    }

    pub(super) fn apply(&mut self, source: &Frame, elapsed: Duration) -> Frame {
        let width = source.size.width as usize;
        let height = source.size.height as usize;
        self.x_band.resize(width, 0);
        self.x_boost.resize(width, 0);
        self.y_boost.resize(height, 0);

        let frame_phase = usize::try_from(elapsed.as_millis() / 33).unwrap_or(usize::MAX);
        let palette_len = DISCO_PALETTE_BGRA.len();
        let horizontal_offset = frame_phase.wrapping_mul(13) % width.max(1);
        let horizontal_sweep = reflected_position(frame_phase.wrapping_mul(23), width);
        let vertical_sweep = reflected_position(frame_phase.wrapping_mul(11), height);
        let horizontal_radius = (width / 7).max(1);
        let vertical_radius = (height / 6).max(1);

        for x in 0..width {
            self.x_band[x] = ((((x + horizontal_offset) % width.max(1)) * palette_len
                / width.max(1)
                + frame_phase / 18)
                % palette_len) as u8;
            self.x_boost[x] = beam_boost(x, horizontal_sweep, horizontal_radius);
        }
        for y in 0..height {
            self.y_boost[y] = beam_boost(y, vertical_sweep, vertical_radius);
        }

        let pulse_position = frame_phase % 60;
        let pulse = pulse_position.min(60 - pulse_position) as u16;
        let base_strength = 76_u16 + pulse * 2;
        let flash_lift = disco_flash_lift(frame_phase);
        let pixels = self
            .frame_pool
            .write_with_fallback_sized(source.pixels().len(), |pixels| {
                pixels.copy_from_slice(source.pixels());
                for y in 0..height {
                    let row_shift =
                        (y * palette_len / height.max(1) + frame_phase / 12) % palette_len;
                    let row_offset = y * source.stride as usize;
                    for x in 0..width {
                        let offset = row_offset + x * 4;
                        let palette =
                            DISCO_PALETTE_BGRA[(self.x_band[x] as usize + row_shift) % palette_len];
                        let strength = (base_strength
                            + u16::from(self.x_boost[x])
                            + u16::from(self.y_boost[y]))
                        .min(188);
                        let inverse = 256 - strength;
                        for channel in 0..3 {
                            let tinted = ((u16::from(pixels[offset + channel]) * inverse
                                + u16::from(palette[channel]) * strength
                                + 128)
                                >> 8) as u8;
                            pixels[offset + channel] = (u16::from(tinted)
                                + ((255 - u16::from(tinted)) * flash_lift + 128) / 256)
                                .min(255)
                                as u8;
                        }
                    }
                }
                paint_disco_sparkles(
                    pixels,
                    source.stride as usize,
                    width,
                    height,
                    frame_phase / 3,
                );
                Ok::<(), std::convert::Infallible>(())
            })
            .expect("disco rendering is infallible");
        Frame::new(
            pixels,
            source.size,
            source.stride,
            source.sequence,
            source.timestamp_100ns,
            source.received_at,
        )
        .expect("disco output preserves the source frame layout")
    }
}

pub(super) fn reflected_position(phase: usize, length: usize) -> usize {
    if length <= 1 {
        return 0;
    }
    let span = length * 2 - 2;
    let position = phase % span;
    if position < length {
        position
    } else {
        span - position
    }
}

pub(super) fn beam_boost(position: usize, center: usize, radius: usize) -> u8 {
    let distance = position.abs_diff(center);
    if distance >= radius {
        0
    } else {
        (((radius - distance) * 48) / radius) as u8
    }
}

pub(super) fn disco_flash_lift(frame_phase: usize) -> u16 {
    let beat = match frame_phase % 36 {
        0 => 30,
        1 => 20,
        2 => 9,
        8 => 22,
        9 => 8,
        _ => 0,
    };
    let major = match frame_phase % 90 {
        0 => 38,
        1 => 26,
        2 => 14,
        3 => 6,
        _ => 0,
    };
    beat.max(major)
}

pub(super) fn paint_disco_sparkles(
    pixels: &mut [u8],
    stride: usize,
    width: usize,
    height: usize,
    phase: usize,
) {
    if width == 0 || height == 0 {
        return;
    }
    for index in 0..18_usize {
        let seed = phase
            .wrapping_mul(1_103_515_245)
            .wrapping_add(index.wrapping_mul(12_345))
            .wrapping_add(0x9e37_79b9);
        let x = seed % width;
        let y = seed.rotate_left(13) % height;
        let radius = 3 + seed.rotate_left(7) % 5;
        for distance in 0..=radius {
            let intensity = 224_u8.saturating_sub((distance * 18) as u8);
            for (sparkle_x, sparkle_y) in [
                (x.saturating_sub(distance), y),
                ((x + distance).min(width - 1), y),
                (x, y.saturating_sub(distance)),
                (x, (y + distance).min(height - 1)),
            ] {
                let offset = sparkle_y * stride + sparkle_x * 4;
                pixels[offset] = pixels[offset].max(intensity);
                pixels[offset + 1] = pixels[offset + 1].max(intensity);
                pixels[offset + 2] = pixels[offset + 2].max(intensity);
            }
        }
    }
}

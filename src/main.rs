use macroquad::prelude::*;

const BALL_R: f32 = 0.28;
const WALL_H: f32 = 0.55;
const BOARD_T: f32 = 0.35;
const HOLE_R: f32 = 0.40;
const CAPTURE_R: f32 = 0.30;
const MAX_TILT: f32 = 0.20; // radians, ~11.5 degrees
const TILT_LERP: f32 = 7.0;
const GRAVITY: f32 = 34.0;
const DAMPING: f32 = 0.50;
const RESTITUTION: f32 = 0.30;
const HOLE_PULL: f32 = 14.0;
const MAX_SPEED: f32 = 11.0;

// '#' wall, 'S' start, 'G' goal hole, 'O' trap hole
const LEVELS: &[&str] = &[
    "\
#########
#S..#...#
##..#.#.#
#...#.#.#
#.###.#.#
#.....#G#
#########",
    "\
###########
#S...#....#
#.##.#.##.#
#.#..#..#.#
#.#.##.O#.#
#.#.....#.#
#.#####.#.#
#......#G.#
###########",
    "\
#############
#S...#......#
#.......O...#
###########.#
#..O........#
#.......#O..#
#.###########
#...........#
#.O...#....G#
#############",
];

struct Level {
    w: i32,
    h: i32,
    walls: Vec<bool>,
    start: Vec2,
    goal: Vec2,
    hazards: Vec<Vec2>,
}

impl Level {
    fn parse(src: &str) -> Level {
        let rows: Vec<&str> = src
            .lines()
            .map(|l| l.trim_end())
            .filter(|l| !l.is_empty())
            .collect();
        let h = rows.len() as i32;
        let w = rows.iter().map(|r| r.chars().count()).max().unwrap() as i32;
        let mut walls = vec![false; (w * h) as usize];
        let mut start = vec2(1.5, 1.5);
        let mut goal = vec2(w as f32 - 1.5, h as f32 - 1.5);
        let mut hazards = Vec::new();
        for (z, row) in rows.iter().enumerate() {
            for (x, c) in row.chars().enumerate() {
                let center = vec2(x as f32 + 0.5, z as f32 + 0.5);
                match c {
                    '#' => walls[z * w as usize + x] = true,
                    'S' => start = center,
                    'G' => goal = center,
                    'O' => hazards.push(center),
                    _ => {}
                }
            }
        }
        Level { w, h, walls, start, goal, hazards }
    }

    fn is_wall(&self, x: i32, z: i32) -> bool {
        if x < 0 || z < 0 || x >= self.w || z >= self.h {
            return true;
        }
        self.walls[(z * self.w + x) as usize]
    }
}

#[derive(Clone, Copy)]
enum Phase {
    Playing,
    Sinking { goal: bool, t: f32, hole: Vec2 },
    Done,
}

fn collide(pos: &mut Vec2, vel: &mut Vec2, level: &Level) {
    let cx = pos.x.floor() as i32;
    let cz = pos.y.floor() as i32;
    // cardinal neighbours first so corners don't snag the ball
    let order = [
        (0, 0), (1, 0), (-1, 0), (0, 1), (0, -1),
        (1, 1), (1, -1), (-1, 1), (-1, -1),
    ];
    for (dx, dz) in order {
        let (wx, wz) = (cx + dx, cz + dz);
        if !level.is_wall(wx, wz) {
            continue;
        }
        let min = vec2(wx as f32, wz as f32);
        let max = min + Vec2::ONE;
        let closest = pos.clamp(min, max);
        let delta = *pos - closest;
        let d2 = delta.length_squared();
        if d2 > 1e-9 && d2 < BALL_R * BALL_R {
            let d = d2.sqrt();
            let n = delta / d;
            *pos += n * (BALL_R - d);
            let vn = vel.dot(n);
            if vn < 0.0 {
                *vel -= n * vn * (1.0 + RESTITUTION);
            }
        }
    }
}

// macroquad has no lighting, so bake a soft top-left highlight into a texture
// to give spheres and cubes some depth
fn gen_shade_texture() -> Texture2D {
    let n: u16 = 128;
    let mut img = Image::gen_image_color(n, n, WHITE);
    for y in 0..n as u32 {
        for x in 0..n as u32 {
            let fx = x as f32 / (n - 1) as f32;
            let fy = y as f32 / (n - 1) as f32;
            let d = ((fx - 0.35).powi(2) + (fy - 0.30).powi(2)).sqrt();
            let b = (1.05 - d * 0.9).clamp(0.45, 1.0);
            img.set_pixel(x, y, Color::new(b, b, b, 1.0));
        }
    }
    Texture2D::from_image(&img)
}

// append one box to the mesh; per-face brightness fakes directional light,
// faces_on = [top, bottom, +z, -z, +x, -x] lets us skip invisible faces
fn add_box(
    verts: &mut Vec<Vertex>,
    idx: &mut Vec<u16>,
    center: Vec3,
    size: Vec3,
    base: Color,
    faces_on: [bool; 6],
) {
    let h = size / 2.0;
    let face_data: [([Vec3; 4], f32); 6] = [
        ([vec3(-h.x, h.y, -h.z), vec3(-h.x, h.y, h.z), vec3(h.x, h.y, h.z), vec3(h.x, h.y, -h.z)], 1.0),
        ([vec3(-h.x, -h.y, -h.z), vec3(h.x, -h.y, -h.z), vec3(h.x, -h.y, h.z), vec3(-h.x, -h.y, h.z)], 0.45),
        ([vec3(-h.x, -h.y, h.z), vec3(h.x, -h.y, h.z), vec3(h.x, h.y, h.z), vec3(-h.x, h.y, h.z)], 0.82),
        ([vec3(h.x, -h.y, -h.z), vec3(-h.x, -h.y, -h.z), vec3(-h.x, h.y, -h.z), vec3(h.x, h.y, -h.z)], 0.60),
        ([vec3(h.x, -h.y, h.z), vec3(h.x, -h.y, -h.z), vec3(h.x, h.y, -h.z), vec3(h.x, h.y, h.z)], 0.70),
        ([vec3(-h.x, -h.y, -h.z), vec3(-h.x, -h.y, h.z), vec3(-h.x, h.y, h.z), vec3(-h.x, h.y, -h.z)], 0.76),
    ];
    let uvs = [vec2(0., 0.), vec2(1., 0.), vec2(1., 1.), vec2(0., 1.)];
    for (fi, (corners, bright)) in face_data.iter().enumerate() {
        if !faces_on[fi] {
            continue;
        }
        let c = Color::new(base.r * bright, base.g * bright, base.b * bright, 1.0);
        let s = verts.len() as u16;
        for (p, uv) in corners.iter().zip(uvs.iter()) {
            verts.push(Vertex::new2(center + *p, *uv, c));
        }
        idx.extend_from_slice(&[s, s + 1, s + 2, s, s + 2, s + 3]);
    }
}

// the whole board + walls as one static mesh: one GPU draw call per frame
// instead of one per wall cell
fn build_level_mesh(level: &Level, shade: &Texture2D) -> Mesh {
    let w = level.w as f32;
    let h = level.h as f32;
    let mut verts = Vec::new();
    let mut idx = Vec::new();

    add_box(
        &mut verts,
        &mut idx,
        vec3(0.0, -BOARD_T / 2.0, 0.0),
        vec3(w, BOARD_T, h),
        Color::from_rgba(198, 166, 112, 255),
        [true, false, true, true, true, true],
    );

    let solid = |x: i32, z: i32| {
        x >= 0 && z >= 0 && x < level.w && z < level.h && level.walls[(z * level.w + x) as usize]
    };
    for z in 0..level.h {
        for x in 0..level.w {
            if !solid(x, z) {
                continue;
            }
            let center = vec3(x as f32 + 0.5 - w / 2.0, WALL_H / 2.0, z as f32 + 0.5 - h / 2.0);
            let faces = [
                true,
                false,
                !solid(x, z + 1),
                !solid(x, z - 1),
                !solid(x + 1, z),
                !solid(x - 1, z),
            ];
            add_box(
                &mut verts,
                &mut idx,
                center,
                vec3(1.0, WALL_H, 1.0),
                Color::from_rgba(150, 106, 68, 255),
                faces,
            );
        }
    }

    Mesh {
        vertices: verts,
        indices: idx,
        texture: Some(shade.clone()),
    }
}

fn draw_hole(x: f32, z: f32, ring: Color) {
    draw_cylinder(vec3(x, 0.010, z), HOLE_R + 0.08, HOLE_R + 0.08, 0.012, None, ring);
    draw_cylinder(vec3(x, 0.024, z), HOLE_R, HOLE_R, 0.012, None, Color::from_rgba(18, 14, 10, 255));
}

// 2x2 box-average downscale so the recorded gif stays small; also flips
// vertically because get_screen_data() returns bottom-up rows
fn downscale_half(bytes: &[u8], w: u32, h: u32) -> (Vec<u8>, u32, u32) {
    let (nw, nh) = (w / 2, h / 2);
    let mut out = Vec::with_capacity((nw * nh * 4) as usize);
    for y in 0..nh {
        let sy = h - 2 - 2 * y;
        for x in 0..nw {
            for c in 0..4u32 {
                let idx = |xx: u32, yy: u32| ((yy * w + xx) * 4 + c) as usize;
                let s = bytes[idx(2 * x, sy)] as u32
                    + bytes[idx(2 * x + 1, sy)] as u32
                    + bytes[idx(2 * x, sy + 1)] as u32
                    + bytes[idx(2 * x + 1, sy + 1)] as u32;
                out.push((s / 4) as u8);
            }
        }
    }
    (out, nw, nh)
}

// autopilot: BFS from the ball's cell to the goal cell (trap cells count as
// blocked) and return the point to steer toward right now
fn bot_target(level: &Level, pos: Vec2) -> Vec2 {
    use std::collections::{HashMap, VecDeque};
    let (sx, sz) = (pos.x.floor() as i32, pos.y.floor() as i32);
    let (gx, gz) = (level.goal.x.floor() as i32, level.goal.y.floor() as i32);
    if (sx, sz) == (gx, gz) {
        return level.goal;
    }
    let trap = |x: i32, z: i32| {
        level
            .hazards
            .iter()
            .any(|h| h.x.floor() as i32 == x && h.y.floor() as i32 == z)
    };
    let mut parent: HashMap<(i32, i32), (i32, i32)> = HashMap::new();
    let mut q = VecDeque::new();
    parent.insert((sx, sz), (sx, sz));
    q.push_back((sx, sz));
    while let Some((cx, cz)) = q.pop_front() {
        if (cx, cz) == (gx, gz) {
            break;
        }
        for (dx, dz) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
            let n = (cx + dx, cz + dz);
            if parent.contains_key(&n) || level.is_wall(n.0, n.1) || trap(n.0, n.1) {
                continue;
            }
            parent.insert(n, (cx, cz));
            q.push_back(n);
        }
    }
    if !parent.contains_key(&(gx, gz)) {
        return level.goal;
    }
    let mut cur = (gx, gz);
    let mut next = cur;
    while cur != (sx, sz) {
        next = cur;
        cur = parent[&cur];
    }
    if next == (gx, gz) {
        level.goal
    } else {
        vec2(next.0 as f32 + 0.5, next.1 as f32 + 0.5)
    }
}

fn center_text(text: &str, y: f32, size: f32, color: Color) {
    let dims = measure_text(text, None, size as u16, 1.0);
    draw_text(text, (screen_width() - dims.width) / 2.0, y, size, color);
}

fn conf() -> Conf {
    Conf {
        window_title: "Tilt Maze".to_owned(),
        window_width: 1100,
        window_height: 820,
        platform: miniquad::conf::Platform {
            swap_interval: Some(1), // ask the driver for vsync
            ..Default::default()
        },
        ..Default::default()
    }
}

#[macroquad::main(conf)]
async fn main() {
    let shade = gen_shade_texture();
    let auto = std::env::args().any(|a| a == "--auto");
    let bot = std::env::args().any(|a| a == "--bot");
    let mut auto_t = 0.0f32;
    let mut frame_no = 0u32;

    // bot mode records its whole run into one animated gif instead of
    // littering the folder with screenshots
    let mut recorder = if bot {
        let file = std::fs::File::create("gameplay.gif").expect("cannot create gameplay.gif");
        let mut enc = image::codecs::gif::GifEncoder::new_with_speed(file, 30);
        let _ = enc.set_repeat(image::codecs::gif::Repeat::Infinite);
        Some(enc)
    } else {
        None
    };

    let mut level_idx = 0usize;
    let mut level = Level::parse(LEVELS[0]);
    let mut board_mesh = build_level_mesh(&level, &shade);
    let mut pos = level.start;
    let mut vel = Vec2::ZERO;
    let mut pitch = 0.0f32;
    let mut roll = 0.0f32;
    let mut phase = Phase::Playing;

    loop {
        let frame_start = get_time();
        let dt = get_frame_time().min(1.0 / 30.0);

        // ---------- input ----------
        if is_key_pressed(KeyCode::Escape) {
            break;
        }
        if is_key_pressed(KeyCode::R) {
            if matches!(phase, Phase::Done) {
                level_idx = 0;
                level = Level::parse(LEVELS[0]);
                board_mesh = build_level_mesh(&level, &shade);
            }
            pos = level.start;
            vel = Vec2::ZERO;
            phase = Phase::Playing;
        }
        if is_key_pressed(KeyCode::N) && !matches!(phase, Phase::Done) {
            level_idx = (level_idx + 1) % LEVELS.len();
            level = Level::parse(LEVELS[level_idx]);
            board_mesh = build_level_mesh(&level, &shade);
            pos = level.start;
            vel = Vec2::ZERO;
            phase = Phase::Playing;
        }

        let mut target_pitch = 0.0;
        let mut target_roll = 0.0;
        if matches!(phase, Phase::Playing) {
            if is_key_down(KeyCode::Up) || is_key_down(KeyCode::W) {
                target_pitch -= MAX_TILT;
            }
            if is_key_down(KeyCode::Down) || is_key_down(KeyCode::S) {
                target_pitch += MAX_TILT;
            }
            if is_key_down(KeyCode::Right) || is_key_down(KeyCode::D) {
                target_roll -= MAX_TILT;
            }
            if is_key_down(KeyCode::Left) || is_key_down(KeyCode::A) {
                target_roll += MAX_TILT;
            }
            if auto {
                // scripted stress input: slam the ball into walls from
                // every direction
                auto_t += dt;
                match ((auto_t / 1.5) as i32) % 4 {
                    0 => target_roll = -MAX_TILT,
                    1 => target_pitch = MAX_TILT,
                    2 => {
                        target_roll = -MAX_TILT;
                        target_pitch = -MAX_TILT;
                    }
                    _ => {
                        target_roll = MAX_TILT;
                        target_pitch = MAX_TILT;
                    }
                }
            }
            if bot {
                let tgt = bot_target(&level, pos);
                let to = tgt - pos;
                let near_trap = level.hazards.iter().any(|h| h.distance(pos) < 1.7);
                let cruise = if near_trap { 1.6 } else { 3.2 };
                let desired = if to.length() > 1e-3 {
                    to.normalize() * cruise
                } else {
                    Vec2::ZERO
                };
                let err = desired - vel;
                target_pitch = (err.y * 0.12).clamp(-MAX_TILT, MAX_TILT);
                target_roll = (-err.x * 0.12).clamp(-MAX_TILT, MAX_TILT);
            }
        }
        let k = (TILT_LERP * dt).min(1.0);
        pitch += (target_pitch - pitch) * k;
        roll += (target_roll - roll) * k;

        // ---------- physics ----------
        match phase {
            Phase::Playing => {
                const STEPS: u32 = 8;
                let h = dt / STEPS as f32;
                for _ in 0..STEPS {
                    let acc = vec2(-GRAVITY * roll.sin(), GRAVITY * pitch.sin());
                    vel += acc * h;
                    vel *= (-DAMPING * h).exp();
                    let speed = vel.length();
                    if speed > MAX_SPEED {
                        vel *= MAX_SPEED / speed;
                    }
                    let prev = pos;
                    pos += vel * h;
                    // two passes so corner wedges (pushed out of one wall
                    // into another) converge instead of tripping the watchdog
                    collide(&mut pos, &mut vel, &level);
                    collide(&mut pos, &mut vel, &level);
                    // tunneling watchdog: never let a substep end with the
                    // ball's center inside a wall cell; keep most momentum so
                    // it reads as a bump, not a freeze
                    if level.is_wall(pos.x.floor() as i32, pos.y.floor() as i32) {
                        pos = prev;
                        vel *= 0.25;
                    }
                }

                let goal_hole = std::iter::once((level.goal, true));
                let trap_holes = level.hazards.iter().map(|&h| (h, false));
                for (hp, is_goal) in goal_hole.chain(trap_holes) {
                    let d = hp - pos;
                    let dist = d.length();
                    if dist > 1e-4 && dist < HOLE_R + BALL_R * 0.5 {
                        vel += d / dist * HOLE_PULL * dt;
                    }
                    if dist < CAPTURE_R {
                        phase = Phase::Sinking { goal: is_goal, t: 0.0, hole: hp };
                    }
                }
            }
            Phase::Sinking { goal, t, hole } => {
                let nt = t + dt * 1.4;
                pos = pos.lerp(hole, (10.0 * dt).min(1.0));
                if nt >= 1.0 {
                    if goal {
                        if level_idx + 1 < LEVELS.len() {
                            level_idx += 1;
                            level = Level::parse(LEVELS[level_idx]);
                            board_mesh = build_level_mesh(&level, &shade);
                            pos = level.start;
                            vel = Vec2::ZERO;
                            phase = Phase::Playing;
                        } else {
                            phase = Phase::Done;
                        }
                    } else {
                        pos = level.start;
                        vel = Vec2::ZERO;
                        phase = Phase::Playing;
                    }
                } else {
                    phase = Phase::Sinking { goal, t: nt, hole };
                }
            }
            Phase::Done => {}
        }

        // ---------- render ----------
        clear_background(Color::from_rgba(24, 26, 38, 255));

        let w = level.w as f32;
        let hh = level.h as f32;
        let span = w.max(hh);
        set_camera(&Camera3D {
            position: vec3(0.0, span * 1.25, span * 0.95),
            target: vec3(0.0, 0.0, 0.0),
            up: vec3(0.0, 1.0, 0.0),
            ..Default::default()
        });

        let rot = Mat4::from_rotation_x(pitch) * Mat4::from_rotation_z(roll);
        {
            let mut gl = unsafe { get_internal_gl() };
            gl.flush();
            gl.quad_gl.push_model_matrix(rot);
        }

        let lx = |x: f32| x - w / 2.0;
        let lz = |z: f32| z - hh / 2.0;

        // board + walls: one pre-built mesh, one draw call
        draw_mesh(&board_mesh);

        // holes
        draw_hole(lx(level.goal.x), lz(level.goal.y), Color::from_rgba(70, 190, 90, 255));
        for hz in &level.hazards {
            draw_hole(lx(hz.x), lz(hz.y), Color::from_rgba(205, 70, 60, 255));
        }

        // ball
        let (sink_y, hide_ball) = match phase {
            Phase::Sinking { t, .. } => (-t * 0.9, t > 0.7),
            Phase::Done => (0.0, true),
            _ => (0.0, false),
        };
        if !hide_ball {
            draw_sphere(
                vec3(lx(pos.x), BALL_R + sink_y, lz(pos.y)),
                BALL_R,
                Some(&shade),
                Color::from_rgba(226, 229, 236, 255),
            );
        }

        {
            let mut gl = unsafe { get_internal_gl() };
            gl.flush();
            gl.quad_gl.pop_model_matrix();
        }

        // ---------- UI overlay ----------
        set_default_camera();
        draw_text(
            &format!("Level {}/{}", level_idx + 1, LEVELS.len()),
            24.0,
            42.0,
            34.0,
            WHITE,
        );
        draw_text(
            "Arrows / WASD: tilt    R: restart    N: skip level    Esc: quit",
            24.0,
            screen_height() - 24.0,
            24.0,
            Color::from_rgba(170, 175, 190, 255),
        );
        if auto || bot {
            draw_text(
                &format!(
                    "fps={} pos=({:.2},{:.2}) vel=({:.2},{:.2}) tilt=({:.3},{:.3})",
                    get_fps(), pos.x, pos.y, vel.x, vel.y, pitch, roll
                ),
                24.0,
                72.0,
                24.0,
                YELLOW,
            );
        }
        match phase {
            Phase::Sinking { goal: true, .. } => {
                center_text("Level complete!", screen_height() * 0.45, 56.0, Color::from_rgba(120, 230, 140, 255));
            }
            Phase::Sinking { goal: false, .. } => {
                center_text("Wrong hole!", screen_height() * 0.45, 56.0, Color::from_rgba(240, 110, 100, 255));
            }
            Phase::Done => {
                center_text("You beat every level!", screen_height() * 0.42, 60.0, Color::from_rgba(255, 215, 110, 255));
                center_text("Press R to play again", screen_height() * 0.50, 32.0, WHITE);
            }
            _ => {}
        }

        if auto {
            frame_no += 1;
            if frame_no % 40 == 0 && frame_no <= 200 {
                let img = get_screen_data();
                img.export_png(&format!("auto_{}.png", frame_no / 40));
            }
            if frame_no > 200 {
                break;
            }
        }
        if bot {
            frame_no += 1;
            let done = matches!(phase, Phase::Done);
            // ~6 fps half-res recording; always capture the final victory frame
            if frame_no % 10 == 0 || done {
                let shot = get_screen_data();
                let (small, w, h) =
                    downscale_half(&shot.bytes, shot.width as u32, shot.height as u32);
                if let (Some(enc), Some(buf)) = (
                    recorder.as_mut(),
                    image::RgbaImage::from_raw(w, h, small),
                ) {
                    let frame = image::Frame::from_parts(
                        buf,
                        0,
                        0,
                        image::Delay::from_numer_denom_ms(167, 1),
                    );
                    let _ = enc.encode_frame(frame);
                }
            }
            if done || frame_no > 9000 {
                drop(recorder.take()); // finalize the gif
                break;
            }
        }

        // pace to 60 fps: uncapped rendering overheats weak GPUs and causes
        // periodic driver stalls; no-op if vsync is already throttling us
        let budget = 1.0 / 60.0;
        let elapsed = get_time() - frame_start;
        if elapsed < budget {
            let remain = budget - elapsed;
            if remain > 0.004 {
                std::thread::sleep(std::time::Duration::from_secs_f64(remain - 0.003));
            }
            while get_time() - frame_start < budget {
                std::hint::spin_loop();
            }
        }

        next_frame().await;
    }
}

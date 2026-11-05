//! CC0 classical bust adapted as Concordia, with a bounded native CPU rasterizer.
//! Real depth-tested geometry: classical bust, swept hair, olive crown and museum plinth.
use crate::config::Rgb;
use image::{Rgba, RgbaImage};
use std::{
    f32::consts::{PI, TAU},
    sync::OnceLock,
};

type V = [f32; 3];
#[derive(Clone, Copy)]
struct Triangle {
    p: [V; 3],
    n: [V; 3],
    ink: f32,
}

fn sub(a: V, b: V) -> V {
    std::array::from_fn(|i| a[i] - b[i])
}
fn cross(a: V, b: V) -> V {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
fn unit(v: V) -> V {
    let n = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2])
        .sqrt()
        .max(0.00001);
    v.map(|x| x / n)
}

fn surface(
    mesh: &mut Vec<Triangle>,
    rows: usize,
    cols: usize,
    ink: f32,
    point: impl Fn(f32, f32) -> V,
) {
    for y in 0..rows {
        for x in 0..cols {
            let u = x as f32 / cols as f32;
            let v = y as f32 / rows as f32;
            let du = 1.0 / cols as f32;
            let dv = 1.0 / rows as f32;
            let [a, b, c, d] = [
                point(u, v),
                point(u + du, v),
                point(u + du, v + dv),
                point(u, v + dv),
            ];
            mesh.push(Triangle {
                p: [a, b, c],
                n: [unit(cross(sub(b, a), sub(c, a))); 3],
                ink,
            });
            mesh.push(Triangle {
                p: [a, c, d],
                n: [unit(cross(sub(c, a), sub(d, a))); 3],
                ink,
            });
        }
    }
}

fn oval(mesh: &mut Vec<Triangle>, center: V, scale: V, ink: f32, tilt: f32) {
    surface(mesh, 7, 12, ink, |u, v| {
        let theta = u * TAU;
        let phi = v * PI;
        let [x, y, z] = [
            theta.sin() * phi.sin() * scale[0],
            phi.cos() * scale[1],
            theta.cos() * phi.sin() * scale[2],
        ];
        [
            center[0] + x * tilt.cos() - y * tilt.sin(),
            center[1] + x * tilt.sin() + y * tilt.cos(),
            center[2] + z,
        ]
    });
}

fn mesh() -> &'static [Triangle] {
    static MESH: OnceLock<Vec<Triangle>> = OnceLock::new();
    MESH.get_or_init(|| {
        // Reviewed fixed compiled asset. No user input, dynamic paths or external URIs.
        let data = include_bytes!("../../assets/concordia.mesh");
        assert_eq!(&data[..4], b"CCM1");
        let count = u32::from_le_bytes(data[4..8].try_into().unwrap()) as usize;
        let faces = u32::from_le_bytes(data[8..12].try_into().unwrap()) as usize;
        assert!(count <= 10000 && faces <= 15000);
        assert_eq!(data.len(), 12 + count * 12 + faces * 6);
        let vertices: Vec<(V, V)> = data[12..12 + count * 12]
            .as_chunks::<12>()
            .0
            .iter()
            .map(|row| {
                let number = |i: usize| {
                    f32::from(i16::from_le_bytes([row[i * 2], row[i * 2 + 1]])) / 32767.0
                };
                (
                    [number(0) * 1.5, number(1) * 1.5, number(2) * 1.5],
                    [number(3), number(4), number(5)],
                )
            })
            .collect();
        let mut mesh = Vec::with_capacity(faces + 4000);
        for face in data[12 + count * 12..].as_chunks::<6>().0 {
            let indices: [usize; 3] = std::array::from_fn(|i| {
                u16::from_le_bytes([face[i * 2], face[i * 2 + 1]]) as usize
            });
            mesh.push(Triangle {
                p: indices.map(|i| vertices[i].0),
                n: indices.map(|i| vertices[i].1),
                ink: 0.95,
            });
        }
        // Concordia's olive wreath is an original addition to the CC0 classical bust.
        for i in 0..18 {
            let theta = -2.55 + i as f32 / 17.0 * 5.1;
            let x = 0.29 * theta.sin();
            let z = 0.34 * theta.cos();
            oval(
                &mut mesh,
                [x, 0.98 + 0.05 * theta.cos(), z],
                [0.034, 0.075, 0.020],
                1.10,
                if x < 0.0 { -0.70 } else { 0.70 },
            );
        }
        mesh
    })
}

fn rotate(p: V, angle: f32) -> V {
    let x = p[0] * angle.cos() + p[2] * angle.sin();
    let z = -p[0] * angle.sin() + p[2] * angle.cos();
    [x, p[1] * 0.993 - z * 0.118, p[1] * 0.118 + z * 0.993]
}

pub(super) fn render(frame: u64, color: Rgb, glitch: bool) -> RgbaImage {
    const SIZE: u32 = 192;
    let mut image = RgbaImage::new(SIZE, SIZE);
    let mut depths = vec![f32::NEG_INFINITY; (SIZE * SIZE) as usize];
    let angle = 0.30 + (frame % 180) as f32 / 180.0 * TAU;
    let light = unit([-0.6, 0.75, 1.0]);
    for triangle in mesh() {
        let p = triangle.p.map(|p| rotate(p, angle));
        let shades = triangle.n.map(|normal| {
            let normal = unit(rotate(normal, angle));
            let diffuse =
                (normal[0] * light[0] + normal[1] * light[1] + normal[2] * light[2]).max(0.0);
            let rim = (1.0 - normal[2].abs()).powi(3) * 0.16;
            (0.18 + 0.78 * diffuse + rim) * triangle.ink
        });
        let screen = p.map(|p| {
            let perspective = 3.9 / (3.9 - p[2]);
            [
                96.0 + p[0] * 72.0 * perspective,
                107.0 - (p[1] - 0.12) * 72.0 * perspective,
                p[2],
            ]
        });
        let edge =
            |a: V, b: V, x: f32, y: f32| (x - a[0]) * (b[1] - a[1]) - (y - a[1]) * (b[0] - a[0]);
        let area = edge(screen[0], screen[1], screen[2][0], screen[2][1]);
        if area.abs() < 0.0001 {
            continue;
        }
        let minx = screen
            .iter()
            .map(|p| p[0])
            .fold(f32::INFINITY, f32::min)
            .floor()
            .max(0.0) as u32;
        let maxx = screen
            .iter()
            .map(|p| p[0])
            .fold(f32::NEG_INFINITY, f32::max)
            .ceil()
            .clamp(0.0, (SIZE - 1) as f32) as u32;
        let miny = screen
            .iter()
            .map(|p| p[1])
            .fold(f32::INFINITY, f32::min)
            .floor()
            .max(0.0) as u32;
        let maxy = screen
            .iter()
            .map(|p| p[1])
            .fold(f32::NEG_INFINITY, f32::max)
            .ceil()
            .clamp(0.0, (SIZE - 1) as f32) as u32;
        for y in miny..=maxy {
            for x in minx..=maxx {
                let a = edge(screen[1], screen[2], x as f32 + 0.5, y as f32 + 0.5) / area;
                let b = edge(screen[2], screen[0], x as f32 + 0.5, y as f32 + 0.5) / area;
                let c = 1.0 - a - b;
                if a < 0.0 || b < 0.0 || c < 0.0 {
                    continue;
                }
                let z = a * screen[0][2] + b * screen[1][2] + c * screen[2][2];
                let index = (y * SIZE + x) as usize;
                if z <= depths[index] {
                    continue;
                }
                depths[index] = z;
                let shade = a * shades[0] + b * shades[1] + c * shades[2];
                let scan = if glitch && y % 3 == 0 { 0.73 } else { 1.0 };
                let rgb = color.map(|channel| {
                    ((f32::from(channel) * shade + (shade - 0.88).max(0.0) * 140.0) * scan)
                        .clamp(0.0, 255.0) as u8
                });
                image.put_pixel(x, y, Rgba([rgb[0], rgb[1], rgb[2], 255]));
            }
        }
    }
    // A brief local signal tear; never touches application text or attachment pixels.
    if glitch && frame % 47 == 31 {
        let original = image.clone();
        for y in 65..70 {
            for x in 0..SIZE {
                image.put_pixel(x, y, *original.get_pixel((x + 5).min(SIZE - 1), y));
            }
        }
    }
    image
}

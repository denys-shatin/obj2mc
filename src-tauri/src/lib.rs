use std::collections::HashSet;
use std::fs::File;
use std::io::BufWriter;
use std::path::Path;
use std::sync::{Arc, Mutex};

use glam::{IVec3, Vec3};
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use ahash::RandomState;
use tauri::Manager;

// ================= СТРУКТУРЫ MINECRAFT =================

#[derive(Serialize, Debug)]
struct McCube {
    origin: [i32; 3],
    size: [i32; 3],
    uv: [i32; 2],
}

#[derive(Serialize, Debug)]
struct McBone {
    name: String,
    pivot: [i32; 3],
    cubes: Vec<McCube>,
}

#[derive(Serialize, Debug)]
struct McGeometry {
    description: McDescription,
    bones: Vec<McBone>,
}

#[derive(Serialize, Debug)]
struct McDescription {
    identifier: String,
    texture_width: i32,
    texture_height: i32,
    visible_bounds_width: i32,
    visible_bounds_height: i32,
    visible_bounds_offset: [i32; 3],
}

#[derive(Serialize, Debug)]
struct OutputRoot {
    format_version: String,
    #[serde(rename = "minecraft:geometry")]
    geometry: Vec<McGeometry>,
}

// ================= TAURI STRUCTS =================

#[derive(Debug, Serialize, Deserialize)]
pub struct FileInfo {
    pub path: String,
    pub name: String,
    pub vertices: usize,
    pub faces: usize,
    pub voxel_count: usize,
    pub cube_count: usize,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ConvertResult {
    pub success: bool,
    pub message: String,
    pub output_path: Option<String>,
    pub voxel_count: usize,
    pub cube_count: usize,
}

// ================= SAT INTERSECTION =================

fn triangle_aabb_intersect(v0: Vec3, v1: Vec3, v2: Vec3, center: Vec3, half_size: f32) -> bool {
    let v0 = v0 - center;
    let v1 = v1 - center;
    let v2 = v2 - center;

    let f0 = v1 - v0;
    let f1 = v2 - v1;
    let f2 = v0 - v2;

    let hs = half_size;

    if v0.x.min(v1.x).min(v2.x) > hs || v0.x.max(v1.x).max(v2.x) < -hs { return false; }
    if v0.y.min(v1.y).min(v2.y) > hs || v0.y.max(v1.y).max(v2.y) < -hs { return false; }
    if v0.z.min(v1.z).min(v2.z) > hs || v0.z.max(v1.z).max(v2.z) < -hs { return false; }

    let normal = f0.cross(f1);
    let d = normal.dot(v0);
    let r = hs * (normal.x.abs() + normal.y.abs() + normal.z.abs());
    if d.abs() > r { return false; }

    let axes = [
        (0.0, -f0.z, f0.y), (0.0, -f1.z, f1.y), (0.0, -f2.z, f2.y),
        (f0.z, 0.0, -f0.x), (f1.z, 0.0, -f1.x), (f2.z, 0.0, -f2.x),
        (-f0.y, f0.x, 0.0), (-f1.y, f1.x, 0.0), (-f2.y, f2.x, 0.0),
    ];

    for (ax, ay, az) in axes {
        let p0 = v0.x * ax + v0.y * ay + v0.z * az;
        let p1 = v1.x * ax + v1.y * ay + v1.z * az;
        let p2 = v2.x * ax + v2.y * ay + v2.z * az;
        
        let r = hs * (ax.abs() + ay.abs() + az.abs());
        if p0.min(p1).min(p2) > r || p0.max(p1).max(p2) < -r {
            return false;
        }
    }

    true
}

// ================= GREEDY MESHING =================

fn run_greedy_meshing(voxels: &HashSet<IVec3, RandomState>) -> Vec<McCube> {
    if voxels.is_empty() { return vec![]; }

    let mut cubes = Vec::new();
    let mut sorted_voxels: Vec<IVec3> = voxels.iter().cloned().collect();
    sorted_voxels.sort_by(|a, b| {
        a.y.cmp(&b.y).then(a.z.cmp(&b.z)).then(a.x.cmp(&b.x))
    });

    let mut processed: HashSet<IVec3, RandomState> = HashSet::default();

    for &pos in &sorted_voxels {
        if processed.contains(&pos) { continue; }

        let (x, y, z) = (pos.x, pos.y, pos.z);
        
        let mut width = 1;
        while voxels.contains(&IVec3::new(x + width, y, z)) 
           && !processed.contains(&IVec3::new(x + width, y, z)) {
            width += 1;
        }

        let mut depth = 1;
        'depth_loop: loop {
            for wx in 0..width {
                let check_pos = IVec3::new(x + wx, y, z + depth);
                if !voxels.contains(&check_pos) || processed.contains(&check_pos) {
                    break 'depth_loop;
                }
            }
            depth += 1;
        }

        let mut height = 1;
        'height_loop: loop {
            for wx in 0..width {
                for dz in 0..depth {
                    let check_pos = IVec3::new(x + wx, y + height, z + dz);
                    if !voxels.contains(&check_pos) || processed.contains(&check_pos) {
                        break 'height_loop;
                    }
                }
            }
            height += 1;
        }

        for wx in 0..width {
            for dz in 0..depth {
                for hy in 0..height {
                    processed.insert(IVec3::new(x + wx, y + hy, z + dz));
                }
            }
        }

        cubes.push(McCube {
            origin: [x, y, z],
            size: [width, height, depth],
            uv: [0, 0],
        });
    }

    cubes
}


// ================= VOXELIZATION =================

fn voxelize_model(models: &[tobj::Model], scale: f32) -> (Vec<McBone>, usize, usize) {
    let voxel_size = 1.0 / scale;
    let half_size = voxel_size / 2.0;

    let bones = Arc::new(Mutex::new(Vec::new()));
    let total_voxels = Arc::new(Mutex::new(0usize));
    let total_cubes = Arc::new(Mutex::new(0usize));

    models.par_iter().for_each(|model| {
        let mesh = &model.mesh;
        if mesh.indices.is_empty() { return; }

        let vertex_vecs: Vec<Vec3> = mesh.positions.chunks(3)
            .map(|v| Vec3::new(v[0], v[1], v[2]))
            .collect();

        let voxels: HashSet<IVec3, RandomState> = mesh.indices.par_chunks(3)
            .map(|chunk| {
                let mut local_voxels = Vec::new();
                let v0 = vertex_vecs[chunk[0] as usize];
                let v1 = vertex_vecs[chunk[1] as usize];
                let v2 = vertex_vecs[chunk[2] as usize];

                let t_min = v0.min(v1).min(v2) * scale;
                let t_max = v0.max(v1).max(v2) * scale;
                
                let i_min = t_min.floor().as_ivec3();
                let i_max = t_max.ceil().as_ivec3();

                for x in i_min.x..=i_max.x {
                    for y in i_min.y..=i_max.y {
                        for z in i_min.z..=i_max.z {
                            let center = Vec3::new(
                                (x as f32 + 0.5) * voxel_size,
                                (y as f32 + 0.5) * voxel_size,
                                (z as f32 + 0.5) * voxel_size
                            );

                            if triangle_aabb_intersect(v0, v1, v2, center, half_size) {
                                local_voxels.push(IVec3::new(x, y, z));
                            }
                        }
                    }
                }
                local_voxels
            })
            .flatten()
            .collect();

        if !voxels.is_empty() {
            let voxel_count = voxels.len();
            let optimized_cubes = run_greedy_meshing(&voxels);
            let cube_count = optimized_cubes.len();
            
            *total_voxels.lock().unwrap() += voxel_count;
            *total_cubes.lock().unwrap() += cube_count;
            
            bones.lock().unwrap().push(McBone {
                name: model.name.clone(),
                pivot: [0, 0, 0],
                cubes: optimized_cubes,
            });
        }
    });

    let final_bones = Arc::try_unwrap(bones).unwrap().into_inner().unwrap();
    let final_voxels = *total_voxels.lock().unwrap();
    let final_cubes = *total_cubes.lock().unwrap();
    
    (final_bones, final_voxels, final_cubes)
}

fn load_obj(path: &str) -> Result<(Vec<tobj::Model>, usize, usize), String> {
    let load_opts = tobj::LoadOptions {
        single_index: true,
        triangulate: true,
        ..Default::default()
    };
    
    let (models, _) = tobj::load_obj(path, &load_opts)
        .map_err(|e| format!("Failed to load OBJ: {}", e))?;

    let mut total_verts = 0;
    let mut total_faces = 0;
    
    for model in &models {
        total_verts += model.mesh.positions.len() / 3;
        total_faces += model.mesh.indices.len() / 3;
    }

    Ok((models, total_verts, total_faces))
}


// ================= TAURI COMMANDS =================

#[tauri::command]
fn analyze_file(path: String, scale: f32) -> Result<FileInfo, String> {
    let (models, vertices, faces) = load_obj(&path)?;
    
    let name = Path::new(&path)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "unknown".to_string());

    let (_, voxel_count, cube_count) = voxelize_model(&models, scale);

    Ok(FileInfo {
        path,
        name,
        vertices,
        faces,
        voxel_count,
        cube_count,
    })
}

#[tauri::command]
fn convert_file(path: String, output_dir: String, scale: f32) -> ConvertResult {
    let (models, _, _) = match load_obj(&path) {
        Ok(v) => v,
        Err(e) => return ConvertResult {
            success: false,
            message: e,
            output_path: None,
            voxel_count: 0,
            cube_count: 0,
        },
    };

    let (bones, voxel_count, cube_count) = voxelize_model(&models, scale);
    
    if bones.is_empty() {
        return ConvertResult {
            success: false,
            message: "No geometry generated".to_string(),
            output_path: None,
            voxel_count: 0,
            cube_count: 0,
        };
    }

    let model_name = Path::new(&path)
        .file_stem()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "model".to_string());

    let output = OutputRoot {
        format_version: "1.12.0".to_string(),
        geometry: vec![McGeometry {
            description: McDescription {
                identifier: format!("geometry.{}", model_name),
                texture_width: 64,
                texture_height: 64,
                visible_bounds_width: 4,
                visible_bounds_height: 4,
                visible_bounds_offset: [0, 1, 0],
            },
            bones,
        }],
    };

    let output_path = Path::new(&output_dir).join(format!("{}.geo.json", model_name));
    let output_str = output_path.to_string_lossy().to_string();

    let file = match File::create(&output_path) {
        Ok(f) => f,
        Err(e) => return ConvertResult {
            success: false,
            message: format!("Failed to create file: {}", e),
            output_path: None,
            voxel_count: 0,
            cube_count: 0,
        },
    };

    let writer = BufWriter::new(file);
    if let Err(e) = serde_json::to_writer_pretty(writer, &output) {
        return ConvertResult {
            success: false,
            message: format!("Failed to write JSON: {}", e),
            output_path: None,
            voxel_count: 0,
            cube_count: 0,
        };
    }

    ConvertResult {
        success: true,
        message: format!("{} voxels → {} cubes", voxel_count, cube_count),
        output_path: Some(output_str),
        voxel_count,
        cube_count,
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![analyze_file, convert_file])
        .setup(|app| {
            let window = app.get_webview_window("main").unwrap();
            window.show().unwrap();
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

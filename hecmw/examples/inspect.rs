use std::path::PathBuf;

fn main() {
    let Some(path) = std::env::args_os().nth(1).map(PathBuf::from) else {
        eprintln!("usage: cargo run -p hecmw --example inspect -- <mesh.msh>");
        std::process::exit(2);
    };

    let mesh = match hecmw::load_mesh_file(&path) {
        Ok(mesh) => mesh,
        Err(error) => {
            eprintln!("failed to load {}: {error}", path.display());
            std::process::exit(1);
        }
    };

    println!("file: {}", path.display());
    println!("nodes: {}", mesh.nodes.len());
    println!("elements: {}", mesh.elements.len());
    println!("cached_edges: {}", mesh.cached_edges().len());
    println!("cached_faces: {}", mesh.cached_faces().len());
    println!(
        "cached_boundary_faces: {}",
        mesh.cached_boundary_faces().len()
    );
    println!(
        "cached_boundary_edges: {}",
        mesh.cached_boundary_edges().len()
    );
    println!("node_sets: {}", mesh.node_sets.len());
    println!("element_sets: {}", mesh.element_sets.len());
    println!("surface_sets: {}", mesh.surface_sets.len());
}

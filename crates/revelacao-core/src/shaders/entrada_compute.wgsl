
// A entrada por compute: um invocation por pixel, escrita numa storage texture.
//
// É o desktop. Concatenada a `corpo.wgsl` por `motor.rs`.
@group(0) @binding(1) var output_texture: texture_storage_2d<rgba8unorm, write>;

@compute @workgroup_size(16, 16)
fn main(@builtin(global_invocation_id) global_id: vec3<u32>) {
    let dims = textureDimensions(input_texture);

    // Os grupos de 16×16 cobrem além da borda; quem cai fora não escreve.
    if (global_id.x >= dims.x || global_id.y >= dims.y) {
        return;
    }

    textureStore(output_texture, vec2<i32>(global_id.xy), revelar_pixel(global_id.xy));
}

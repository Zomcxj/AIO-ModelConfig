fn main() {
    let python = "python";
    if std::path::Path::new("icon.png").exists() {
        let output = std::process::Command::new(python)
            .args([
                "-c",
                r#"
from PIL import Image
img = Image.open("icon.png").convert("RGBA")
img.thumbnail((256, 256), Image.LANCZOS)
img.save("icon.ico")
w, h = img.size
if w != 256 or h != 256:
    new_img = Image.new("RGBA", (256, 256), (0, 0, 0, 0))
    new_img.paste(img, ((256 - w) // 2, (256 - h) // 2))
    img = new_img
img = img.resize((256, 256), Image.LANCZOS)
with open("icon_rgba.bin", "wb") as f:
    f.write(img.tobytes())
print("icon ready:", len(img.tobytes()))
"#,
            ])
            .output()
            .unwrap();
        if !output.status.success() {
            eprintln!(
                "icon.png conversion failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
    } else {
        let (big_w, big_h) = (256usize, 256usize);
        let mut big_rgba = vec![0u8; big_w * big_h * 4];
        let bcx = big_w as f32 / 2.0;
        let bcy = big_h as f32 / 2.0;
        let outer_r = 108.0;
        let inner_r = 48.0;
        for y in 0..big_h {
            for x in 0..big_w {
                let dx = x as f32 - bcx;
                let dy = y as f32 - bcy;
                let dist = (dx * dx + dy * dy).sqrt();
                if dist <= outer_r && dist >= inner_r {
                    let idx = (y * big_w + x) * 4;
                    big_rgba[idx] = 255;
                    big_rgba[idx + 1] = 255;
                    big_rgba[idx + 2] = 255;
                    big_rgba[idx + 3] = 255;
                }
            }
        }
        std::fs::write("icon_rgba.bin", &big_rgba).unwrap();
    }

    let mut res = winres::WindowsResource::new();
    res.set_icon("icon.ico");
    res.compile().unwrap();
}
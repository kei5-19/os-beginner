use core::{
    ops::{Add, AddAssign, BitAnd, Sub, SubAssign},
    slice,
};

use crate::{console::DESKTOP_BG_COLOR, frame_buffer_config::FrameBufferConfig, util::OnceStatic};

/// フレームバッファ情報。
pub static FB_CONFIG: OnceStatic<FrameBufferConfig> = OnceStatic::new();

#[derive(PartialEq, Eq, Clone, Default, Copy)]
pub struct PixelColor {
    r: u8,
    g: u8,
    b: u8,
}

/// ピクセルの色情報を持つ。
impl PixelColor {
    /// 初期化。
    pub const fn new(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b }
    }

    /// 32 bit 情報から [PixelColor] へ変換する。
    pub fn to_color(c: u32) -> Self {
        Self {
            r: (c >> 16) as u8,
            g: (c >> 8) as u8,
            b: c as u8,
        }
    }
}

/// ピクセルを塗るための色々を提供する。
pub trait PixelWrite {
    /// ピクセルを塗る手段を提供する。
    fn write(&mut self, pos: Vector2D<i32>, color: &PixelColor);

    /// フレームバッファの先頭アドレスを表す。
    fn frame_buffer(&self) -> usize;

    /// 1行あたりのピクセル数を表す。
    fn pixels_per_scan_line(&self) -> usize;

    /// 横方向の解像度を表す。
    fn horizontal_resolution(&self) -> usize;

    /// 縦方向の解像度を表す。
    fn vertical_resolution(&self) -> usize;

    /// 長方形の枠を指定された色で塗る。
    fn draw_rectangle(&mut self, pos: Vector2D<i32>, size: Vector2D<i32>, c: &PixelColor) {
        // 横線
        for dx in 0..size.x {
            self.write(pos + Vector2D::new(dx, 0), c);
            self.write(pos + Vector2D::new(dx, size.y - 1), c);
        }

        // 縦線
        for dy in 0..size.y {
            self.write(pos + Vector2D::new(0, dy), c);
            self.write(pos + Vector2D::new(size.x - 1, dy), c);
        }
    }

    // 長方形を指定された色で塗る。
    fn fill_rectangle(&mut self, pos: Vector2D<i32>, size: Vector2D<i32>, c: &PixelColor) {
        for dy in 0..size.y {
            for dx in 0..size.x {
                self.write(pos + Vector2D::new(dx, dy), c);
            }
        }
    }
}

/// フレームバッファのピクセルの持ち方が RGB のときのクラス。
pub struct RgbResv8BitPerColorPixelWriter {
    config: FrameBufferConfig,
}

impl RgbResv8BitPerColorPixelWriter {
    pub fn new(config: FrameBufferConfig) -> Self {
        Self { config }
    }

    fn pixel_at(&mut self, pos: Vector2D<i32>) -> Option<&mut [u8; 3]> {
        if !(0..self.config.pixels_per_scan_line as i32).contains(&pos.x())
            || !(0..self.config.vertical_resolution as i32).contains(&pos.y())
        {
            return None;
        }
        unsafe {
            Some(
                slice::from_raw_parts_mut(
                    (self.frame_buffer()
                        + 4 * (self.pixels_per_scan_line() * pos.y as usize + pos.x as usize))
                        as *mut u8,
                    3,
                )
                .try_into()
                .unwrap(),
            )
        }
    }
}

impl PixelWrite for RgbResv8BitPerColorPixelWriter {
    fn write(&mut self, pos: Vector2D<i32>, color: &PixelColor) {
        if let Some(pixel) = self.pixel_at(pos) {
            pixel[0] = color.r;
            pixel[1] = color.g;
            pixel[2] = color.b;
        }
    }

    fn frame_buffer(&self) -> usize {
        self.config.frame_buffer
    }

    fn pixels_per_scan_line(&self) -> usize {
        self.config.pixels_per_scan_line
    }

    fn horizontal_resolution(&self) -> usize {
        self.config.horizontal_resolution
    }

    fn vertical_resolution(&self) -> usize {
        self.config.vertical_resolution
    }
}

/// フレームバッファのピクセルの持ち方が BGR のときのクラス。
pub struct BgrResv8BitPerColorPixelWriter {
    config: FrameBufferConfig,
}

impl BgrResv8BitPerColorPixelWriter {
    /// 初期化。
    pub fn new(config: FrameBufferConfig) -> Self {
        Self { config }
    }

    fn pixel_at(&mut self, pos: Vector2D<i32>) -> Option<&mut [u8; 3]> {
        if !(0..self.config.pixels_per_scan_line as i32).contains(&pos.x())
            || !(0..self.config.vertical_resolution as i32).contains(&pos.y())
        {
            return None;
        }
        unsafe {
            Some(
                slice::from_raw_parts_mut(
                    (self.frame_buffer()
                        + 4 * (self.pixels_per_scan_line() * pos.y as usize + pos.x as usize))
                        as *mut u8,
                    3,
                )
                .try_into()
                .unwrap(),
            )
        }
    }
}

impl PixelWrite for BgrResv8BitPerColorPixelWriter {
    fn write(&mut self, pos: Vector2D<i32>, color: &PixelColor) {
        if let Some(pixel) = self.pixel_at(pos) {
            pixel[0] = color.b;
            pixel[1] = color.g;
            pixel[2] = color.r;
        }
    }

    fn frame_buffer(&self) -> usize {
        self.config.frame_buffer
    }

    fn pixels_per_scan_line(&self) -> usize {
        self.config.pixels_per_scan_line
    }

    fn horizontal_resolution(&self) -> usize {
        self.config.horizontal_resolution
    }

    fn vertical_resolution(&self) -> usize {
        self.config.vertical_resolution
    }
}

#[derive(Debug, PartialEq, Eq, Clone, Copy, Default)]
/// 2次元のベクトル情報を保持するクラス。
pub struct Vector2D<T> {
    x: T,
    y: T,
}

impl<T> Vector2D<T> {
    /// 初期化。
    pub const fn new(x: T, y: T) -> Self {
        Self { x, y }
    }
}

impl<T: Copy> Vector2D<T> {
    /// x 成分を返す。
    pub const fn x(&self) -> T {
        self.x
    }

    /// y 成分を返す。
    pub const fn y(&self) -> T {
        self.y
    }
}

impl<T: Ord + Copy> Vector2D<T> {
    /// 与えられた2つの各 x, y 要素のうち、最大のものを
    /// 新たな x, y 要素としたものを返す。
    pub fn element_max(lhs: &Self, rhs: &Self) -> Self {
        use core::cmp::max;

        Self {
            x: max(lhs.x, rhs.x),
            y: max(lhs.y, rhs.y),
        }
    }

    /// 与えられた2つの各 x, y 要素のうち、最小のものを
    /// 新たな x, y 要素としたものを返す。
    pub fn element_min(lhs: &Self, rhs: &Self) -> Self {
        use core::cmp::min;

        Self {
            x: min(lhs.x, rhs.x),
            y: min(lhs.y, rhs.y),
        }
    }
}

/// 成分の加算を加算として定義する。
impl<T> Add for Vector2D<T>
where
    T: Add<Output = T>,
{
    type Output = Self;
    fn add(self, rhs: Self) -> Self::Output {
        Self {
            x: self.x + rhs.x,
            y: self.y + rhs.y,
        }
    }
}

/// 成分の加算を加算として定義する。
impl<T: AddAssign> AddAssign for Vector2D<T> {
    fn add_assign(&mut self, rhs: Self) {
        self.x += rhs.x;
        self.y += rhs.y;
    }
}

/// 成分の減算を減算として定義する。
impl<T> Sub for Vector2D<T>
where
    T: Sub<Output = T>,
{
    type Output = Self;
    fn sub(self, rhs: Self) -> Self::Output {
        Self {
            x: self.x - rhs.x,
            y: self.y - rhs.y,
        }
    }
}

/// 成分の減算を減算として定義する。
impl<T: SubAssign> SubAssign for Vector2D<T> {
    fn sub_assign(&mut self, rhs: Self) {
        self.x -= rhs.x;
        self.y -= rhs.y;
    }
}

#[derive(PartialEq, Eq, Clone, Copy)]
pub struct Rectangle<T> {
    pub pos: Vector2D<T>,
    pub size: Vector2D<T>,
}

impl<T> BitAnd for Rectangle<T>
where
    T: Add<Output = T> + Sub<Output = T> + Ord + Default + Copy,
{
    type Output = Self;
    fn bitand(self, rhs: Self) -> Self::Output {
        use core::cmp::{max, min};

        let self_end = self.pos + self.size;
        let rhs_end = rhs.pos + rhs.size;

        if self_end.x < rhs.pos.x
            || self_end.y < rhs.pos.y
            || rhs_end.x < self.pos.x
            || rhs_end.y < self.pos.y
        {
            return Self {
                pos: Vector2D {
                    ..Default::default()
                },
                size: Vector2D {
                    ..Default::default()
                },
            };
        }

        let new_pos = Vector2D {
            x: max(self.pos.x, rhs.pos.x),
            y: max(self.pos.y, rhs.pos.y),
        };
        let new_size = Vector2D {
            x: min(self_end.x, rhs_end.x) - new_pos.x,
            y: min(self_end.y, rhs_end.y) - new_pos.y,
        };
        Self {
            pos: new_pos,
            size: new_size,
        }
    }
}

/// デスクトップ背景を描画する。
pub fn draw_desktop(writer: &mut dyn PixelWrite) {
    let frame_width = writer.horizontal_resolution() as i32;
    let frame_height = writer.vertical_resolution() as i32;

    // デスクトップ背景の描画
    writer.fill_rectangle(
        Vector2D::new(0, 0),
        Vector2D::new(frame_width, frame_height - 50),
        &DESKTOP_BG_COLOR,
    );
    // タスクバーの表示
    writer.fill_rectangle(
        Vector2D::new(0, frame_height - 50),
        Vector2D::new(frame_width, 50),
        &PixelColor::new(1, 8, 17),
    );
    // （多分）Windows の検索窓
    writer.fill_rectangle(
        Vector2D::new(0, frame_height - 50),
        Vector2D::new(frame_width / 5, 50),
        &PixelColor::new(80, 80, 80),
    );
    // （多分）Windows のスタートボタン
    writer.fill_rectangle(
        Vector2D::new(10, frame_height - 40),
        Vector2D::new(30, 30),
        &PixelColor::new(160, 160, 160),
    );
}

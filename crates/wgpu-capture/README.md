# wgpu-capture
This library was created to support Harmony's screen sharing feature, which requires efficient GPU capture, display, and encoding pipelines on all supported platforms. The crate provides a unified API for capturing the screen, importing frames into `wgpu` textures for display, and encoding frames to H.264/AV1 with hardware acceleration.

## Goals
* Avoid GPU copies whenever possible
* Native hardware accelerated encoding using `libva` on Linux and Media Foundation on Windows (no FFmpeg/GStreamer)
* Display previews by importing frames into `wgpu` textures (supported by `iced`)

## To do
* Implement monitor/window enumeration on Windows

use super::*;


    fn adapter(name: &str, device_type: wgpu::DeviceType) -> GpuAdapter {
        GpuAdapter {
            name: name.to_string(),
            backend: wgpu::Backend::Vulkan,
            device_type,
        }
    }

    #[test]
    fn software_is_the_device_type_or_a_known_fallback_name() {
        assert!(adapter("llvmpipe (LLVM 21.1.8, 256 bits)", wgpu::DeviceType::Cpu).is_software());
        // WebGPU reports every adapter as Other, so the name has to carry it.
        assert!(adapter(
            "ANGLE (Google, Vulkan 1.3.0 (SwiftShader Device (Subzero)), SwiftShader driver)",
            wgpu::DeviceType::Other
        )
        .is_software());
        assert!(adapter("Microsoft Basic Render Driver", wgpu::DeviceType::Other).is_software());
        assert!(!adapter("NVIDIA GeForce GT 1030", wgpu::DeviceType::DiscreteGpu).is_software());
        assert!(!adapter("Apple M2", wgpu::DeviceType::IntegratedGpu).is_software());
        assert!(!adapter("", wgpu::DeviceType::Other).is_software());
    }

    #[test]
    fn the_verdict_needs_either_a_device_or_enough_dropped_draws() {
        assert_eq!(gpu_status_from(None, 0), GpuStatus::Unknown);
        assert_eq!(
            gpu_status_from(None, SCENE_DRAWS_BEFORE_NO_RENDERER - 1),
            GpuStatus::Unknown
        );
        assert_eq!(
            gpu_status_from(None, SCENE_DRAWS_BEFORE_NO_RENDERER),
            GpuStatus::NoRenderer
        );
        let gpu = adapter("NVIDIA GeForce GT 1030", wgpu::DeviceType::DiscreteGpu);
        assert_eq!(
            gpu_status_from(Some(gpu.clone()), 100),
            GpuStatus::Hardware(gpu)
        );
        let cpu = adapter("llvmpipe", wgpu::DeviceType::Cpu);
        assert_eq!(
            gpu_status_from(Some(cpu.clone()), 0),
            GpuStatus::Software(cpu)
        );
    }

    #[test]
    fn only_degraded_verdicts_carry_an_identity() {
        assert!(GpuStatus::Unknown.identity().is_none());
        assert!(
            GpuStatus::Hardware(adapter("x", wgpu::DeviceType::DiscreteGpu))
                .identity()
                .is_none()
        );
        assert_eq!(
            GpuStatus::NoRenderer.identity().as_deref(),
            Some("no-renderer")
        );
        let software = GpuStatus::Software(adapter("llvmpipe", wgpu::DeviceType::Cpu));
        assert!(software.is_degraded());
        assert_eq!(software.identity().as_deref(), Some("software:llvmpipe"));
    }

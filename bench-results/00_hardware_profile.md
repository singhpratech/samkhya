# Hardware profile

Captured: 2026-05-16 (UTC). Sole author: Prateek Singh. License: Apache-2.0.

This document is the fixed-point reference for every benchmark in `bench-results/`.
All performance numbers must be interpreted relative to this hardware.

## CPU

```
$ lscpu
Architecture:                            x86_64
CPU op-mode(s):                          32-bit, 64-bit
Address sizes:                           39 bits physical, 48 bits virtual
Byte Order:                              Little Endian
CPU(s):                                  20
On-line CPU(s) list:                     0-19
Vendor ID:                               GenuineIntel
Model name:                              13th Gen Intel(R) Core(TM) i9-13900HK
CPU family:                              6
Model:                                   186
Thread(s) per core:                      2
Core(s) per socket:                      14
Socket(s):                               1
Stepping:                                2
CPU(s) scaling MHz:                      18%
CPU max MHz:                             5400.0000
CPU min MHz:                             400.0000
BogoMIPS:                                5990.40
Flags:                                   fpu vme de pse tsc msr pae mce cx8 apic sep mtrr pge mca cmov pat
                                         pse36 clflush dts acpi mmx fxsr sse sse2 ss ht tm pbe syscall nx
                                         pdpe1gb rdtscp lm constant_tsc art arch_perfmon pebs bts rep_good
                                         nopl xtopology nonstop_tsc cpuid aperfmperf tsc_known_freq pni
                                         pclmulqdq dtes64 monitor ds_cpl vmx smx est tm2 ssse3 sdbg fma
                                         cx16 xtpr pdcm pcid sse4_1 sse4_2 x2apic movbe popcnt
                                         tsc_deadline_timer aes xsave avx f16c rdrand lahf_lm abm
                                         3dnowprefetch cpuid_fault epb ssbd ibrs ibpb stibp ibrs_enhanced
                                         tpr_shadow flexpriority ept vpid ept_ad fsgsbase tsc_adjust bmi1
                                         avx2 smep bmi2 erms invpcid rdseed adx smap clflushopt clwb
                                         intel_pt sha_ni xsaveopt xsavec xgetbv1 xsaves split_lock_detect
                                         user_shstk avx_vnni dtherm ida arat pln pts hwp hwp_notify
                                         hwp_act_window hwp_epp hwp_pkg_req hfi vnmi umip pku ospke
                                         waitpkg gfni vaes vpclmulqdq rdpid movdiri movdir64b fsrm
                                         md_clear serialize arch_lbr ibt flush_l1d arch_capabilities
Virtualization:                          VT-x
L1d cache:                               544 KiB (14 instances)
L1i cache:                               704 KiB (14 instances)
L2 cache:                                11.5 MiB (8 instances)
L3 cache:                                24 MiB (1 instance)
NUMA node(s):                            1
NUMA node0 CPU(s):                       0-19
Vulnerability Gather data sampling:      Not affected
Vulnerability Ghostwrite:                Not affected
Vulnerability Indirect target selection: Not affected
Vulnerability Itlb multihit:             Not affected
Vulnerability L1tf:                      Not affected
Vulnerability Mds:                       Not affected
Vulnerability Meltdown:                  Not affected
Vulnerability Mmio stale data:           Not affected
Vulnerability Old microcode:             Not affected
Vulnerability Reg file data sampling:    Mitigation; Clear Register File
Vulnerability Retbleed:                  Not affected
Vulnerability Spec rstack overflow:      Not affected
Vulnerability Spec store bypass:         Mitigation; Speculative Store Bypass disabled via prctl
Vulnerability Spectre v1:                Mitigation; usercopy/swapgs barriers and __user pointer sanitization
Vulnerability Spectre v2:                Mitigation; Enhanced / Automatic IBRS; IBPB conditional; PBRSB-eIBRS SW sequence; BHI BHI_DIS_S
Vulnerability Srbds:                     Not affected
Vulnerability Tsa:                       Not affected
Vulnerability Tsx async abort:           Not affected
Vulnerability Vmscape:                   Mitigation; IBPB before exit to userspace
```

```
$ grep -E "model name|cpu cores|siblings|cache size" /proc/cpuinfo | sort -u
cache size	: 24576 KB
cpu cores	: 14
model name	: 13th Gen Intel(R) Core(TM) i9-13900HK
siblings	: 20
```

```
$ nproc
20
```

Cores: 14 physical / 20 logical (SMT/Hyper-Threading enabled: yes; 2 threads per core).
The i9-13900HK is a hybrid (P-core + E-core) part: 6 P-cores (with SMT -> 12 threads) + 8 E-cores (no SMT -> 8 threads) = 20 logical CPUs total. This explains the asymmetric `L2 cache: 11.5 MiB (8 instances)` (6 P-core L2 slices + 2 E-core L2 clusters).
Base / boost / cache: min 400 MHz, max 5400 MHz (P-core turbo); L1d 544 KiB total, L1i 704 KiB total, L2 11.5 MiB total, L3 24 MiB shared.
Relevant ISA: AVX2, AVX-VNNI, F16C, AES, SHA-NI, BMI1/2 (no AVX-512).

## Memory

```
$ free -h
               total        used        free      shared  buff/cache   available
Mem:            31Gi       5.3Gi       1.4Gi       180Mi        24Gi        25Gi
Swap:          2.0Gi       2.0Gi        43Mi
```

```
$ head -5 /proc/meminfo
MemTotal:       32536456 kB
MemFree:         1475840 kB
MemAvailable:   26948020 kB
Buffers:         1305824 kB
Cached:         23337928 kB
```

Total RAM: ~31 GiB (32,536,456 kB ≈ 31.03 GiB). Swap: 2 GiB (nearly full at capture time — may indicate elevated memory pressure during this snapshot).

## GPU

`nvidia-smi` itself FAILED at capture time with a driver/library version mismatch:

```
$ nvidia-smi
Failed to initialize NVML: Driver/library version mismatch
NVML library version: 580.159
```

```
$ nvidia-smi --query-gpu=name,memory.total,memory.free,driver_version,cuda_version,compute_cap,clocks.gr,clocks.mem,power.limit --format=csv
Failed to initialize NVML: Driver/library version mismatch
NVML library version: 580.159
```

The mismatch is between the running kernel module and the userspace NVML library (both are 580.x but differ at the patch level — kernel reboot required to reload the newer module). The GPU itself is fully present and visible via fallback sources:

```
$ lspci | grep -i -E "vga|3d|nvidia"
00:02.0 VGA compatible controller: Intel Corporation Raptor Lake-P [Iris Xe Graphics] (rev 04)
01:00.0 VGA compatible controller: NVIDIA Corporation GN21-X11 (rev a1)
01:00.1 Audio device: NVIDIA Corporation Device 22bb (rev a1)
```

```
$ cat /proc/driver/nvidia/version
NVRM version: NVIDIA UNIX Open Kernel Module for x86_64  580.142  Release Build  (dvs-builder@U22-I3-AD05-51-5)  Tue Mar  3 19:14:46 UTC 2026
GCC version:  gcc version 13.3.0 (Ubuntu 13.3.0-6ubuntu2~24.04.1)
```

```
$ ls /usr/lib/x86_64-linux-gnu/libnvidia-ml.so*
/usr/lib/x86_64-linux-gnu/libnvidia-ml.so.1
/usr/lib/x86_64-linux-gnu/libnvidia-ml.so.580.159.04
```

```
$ nvcc --version
nvcc: NVIDIA (R) Cuda compiler driver
Copyright (c) 2005-2025 NVIDIA Corporation
Built on Wed_Aug_20_01:58:59_PM_PDT_2025
Cuda compilation tools, release 13.0, V13.0.88
```

| metric | value | source |
| --- | --- | --- |
| Model (discrete) | NVIDIA GN21-X11 (RTX 40-series Mobile, Ada Lovelace) | `lspci` (PCI ID `10de:` rev a1) |
| Model (integrated) | Intel Iris Xe Graphics (Raptor Lake-P) | `lspci` |
| VRAM | unverified at capture (NVML offline); nominal for GN21-X11 SKU: 16 GB GDDR6 | nominal; not measured |
| Compute capability | sm_89 (Ada Lovelace, unverified at capture) | inferred from GN21-X11 |
| CUDA toolkit | 13.0, V13.0.88 (build 2025-08-20) | `nvcc --version` |
| Driver (kernel module) | 580.142 (NVIDIA Open Kernel Module) | `/proc/driver/nvidia/version` |
| Driver (NVML userspace) | 580.159.04 — **mismatched against kernel** | `libnvidia-ml.so.580.159.04` |
| Graphics clock | unverified at capture (NVML offline) | n/a |
| Memory clock | unverified at capture (NVML offline) | n/a |
| Power limit | unverified at capture (NVML offline) | n/a |

**Action required before any GPU benchmark**: reboot to reload the matching kernel module, then re-capture clocks / VRAM / power limit and update this section. Until then, treat any GPU number as provisional.

## Storage

```
$ df -h /home
Filesystem      Size  Used Avail Use% Mounted on
/dev/nvme0n1p5  1.1T  885G  107G  90% /
```

```
$ lsblk -d -o NAME,SIZE,ROTA,MODEL
NAME     SIZE ROTA MODEL
sda        0B    0 SD/MMC CRW
nvme0n1  1.9T    0 PC801 NVMe SK hynix 2TB
```

Primary disk: SK hynix PC801 1.9 TB NVMe SSD (`ROTA=0` confirms solid-state). Root partition `/dev/nvme0n1p5` is 1.1 TiB, 90 % full (107 GiB free). Bench artifacts should stay well under this margin or move to an external disk.

## OS + toolchain

```
$ uname -a
Linux <host> 6.17.0-23-generic #23~24.04.1-Ubuntu SMP PREEMPT_DYNAMIC Tue Apr 14 16:11:48 UTC 2 x86_64 x86_64 x86_64 GNU/Linux
```

```
$ head -6 /etc/os-release
NAME="Linux Mint"
VERSION="22.3 (Zena)"
ID=linuxmint
ID_LIKE="ubuntu debian"
PRETTY_NAME="Linux Mint 22.3"
VERSION_ID="22.3"
```

```
$ rustc --version
rustc 1.94.1 (e408947bf 2026-03-25)
$ cargo --version
cargo 1.94.1 (29ea6fb6a 2026-03-24)
$ python3 --version
Python 3.12.3
```

Kernel: 6.17.0-23-generic (Ubuntu 24.04 LTS HWE stack on Linux Mint 22.3 "Zena").
Rust: 1.94.1 (stable, March 2026).
Python: 3.12.3.

## Idle thermal state

```
$ nvidia-smi --query-gpu=temperature.gpu,power.draw,utilization.gpu,utilization.memory --format=csv,noheader
Failed to initialize NVML: Driver/library version mismatch
NVML library version: 580.159
```

Unavailable at capture (same NVML mismatch as above). Must be re-captured after reboot before any thermal-stability claim is made.

## Caveats for interpreting numbers

- Single-machine, single-run. No replication across hardware.
- This is a laptop CPU (i9-13900HK) on a laptop chassis — CPU thermal throttling is likely at sustained 100 % utilization on all 20 threads; expect lower steady-state than 5.4 GHz turbo.
- Hybrid P/E architecture: pinning matters. Untuned multi-threaded benchmarks will see asymmetric per-thread throughput (P-cores ~2× E-cores) and the scheduler's HFI hints can migrate threads mid-run.
- No AVX-512 on this part. SIMD baselines should target AVX2 + AVX-VNNI.
- GPU is the discrete laptop GN21-X11 (RTX 40-series Mobile); a mobile Ada part is power- and TGP-limited compared to its desktop namesake. Do not extrapolate desktop-RTX numbers from these results.
- GPU clocks may boost transiently; treat power-limited steady-state as the baseline (must re-measure after reboot — NVML was offline at capture).
- Reported wall-clock times include kernel-launch overhead for GPU paths.
- Root filesystem was 90 % full and swap was 2.0 GiB used out of 2.0 GiB at capture time — re-verify before long benchmark runs to avoid swap-driven distortion.
- All paths in published bench artifacts are anonymised to `<repo>/`; hostname is redacted to `<host>`.

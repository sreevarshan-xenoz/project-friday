using FridayTray.Core;
using Xunit;

namespace FridayTray.Tests;

public class LocalAiTests
{
    // Real sample output from `cargo run -q -p friday-mobile --bin friday-ctl -- local-ai detect`
    private const string RealDetectOutput = """
        ram_mb=15764
        gpu=nvidia
        gpu_discrete=yes
        vram_mb=unknown
        cpu_threads=8
        avx2=yes
        disk_free_mb=162023
        """;

    // Real sample output from `cargo run -q -p friday-mobile --bin friday-ctl -- local-ai presets --json`
    private const string RealPresetsJson = """
        [
          {
            "available": true,
            "budget_mb": 3941,
            "download_mb": 2497,
            "fast": false,
            "id": "light",
            "label": "Light (up to 3.8 GB of memory)",
            "model": "qwen3:4b",
            "note": "needs about 3.5 GB while answering, text only: can't see screenshots, backs up cloud (cloud answers first while online)",
            "vision": false
          },
          {
            "available": true,
            "budget_mb": 7882,
            "download_mb": 3350,
            "fast": false,
            "id": "balanced",
            "label": "Balanced (up to 7.7 GB of memory)",
            "model": "gemma3:4b",
            "note": "needs about 4.5 GB while answering, sees screenshots, backs up cloud (cloud answers first while online)",
            "vision": true
          },
          {
            "available": true,
            "budget_mb": 11823,
            "download_mb": 8164,
            "fast": false,
            "id": "most",
            "label": "Most (up to 11.5 GB of memory)",
            "model": "gemma3:12b",
            "note": "needs about 9 GB while answering, sees screenshots, answers first",
            "vision": true
          }
        ]
        """;

    [Fact]
    public void Parses_real_sample_detect_and_presets_output()
    {
        var info = LocalAi.Parse(RealDetectOutput, RealPresetsJson);

        Assert.Equal("NVIDIA GPU", info.GpuName);
        Assert.Null(info.VramMb);
        Assert.Equal(15764, info.RamMb);
        Assert.Equal("balanced", info.RecommendedPreset);
        Assert.Equal("gemma3:4b", info.RecommendedModel);
        Assert.True(info.Vision);
        Assert.True(info.HasModel);
        Assert.Equal("sees screenshots", info.VisionSummary);
        Assert.Equal("NVIDIA GPU, 15.4 GB RAM", info.MachineSummary);
    }

    [Fact]
    public void Parses_machine_with_discrete_gpu_and_vram()
    {
        var detect = """
            ram_mb=32768
            gpu=NVIDIA RTX 3060
            gpu_discrete=yes
            vram_mb=12288
            cpu_threads=16
            avx2=yes
            disk_free_mb=250000
            """;

        var presets = """
            [
              {
                "available": true,
                "budget_mb": 11776,
                "download_mb": 8164,
                "fast": true,
                "id": "gpu",
                "label": "GPU (up to 11.5 GB of video memory)",
                "model": "gemma3:12b",
                "note": "needs about 9 GB while answering, sees screenshots, answers first",
                "vision": true
              },
              {
                "available": true,
                "budget_mb": 8192,
                "download_mb": 3350,
                "fast": false,
                "id": "balanced",
                "label": "Balanced (up to 8 GB of memory)",
                "model": "gemma3:4b",
                "note": "needs about 4.5 GB while answering, sees screenshots, backs up cloud",
                "vision": true
              }
            ]
            """;

        var info = LocalAi.Parse(detect, presets);

        Assert.Equal("NVIDIA RTX 3060", info.GpuName);
        Assert.Equal(12288, info.VramMb);
        Assert.Equal(32768, info.RamMb);
        Assert.Equal("gpu", info.RecommendedPreset);
        Assert.Equal("gemma3:12b", info.RecommendedModel);
        Assert.True(info.Vision);
        Assert.Equal("NVIDIA RTX 3060, 12 GB VRAM, 32 GB RAM", info.MachineSummary);
    }

    [Fact]
    public void Handles_text_only_model()
    {
        var detect = "ram_mb=8192\ngpu=none\ngpu_discrete=no\nvram_mb=unknown\n";
        var presets = """
            [
              {
                "available": true,
                "budget_mb": 2048,
                "download_mb": 1834,
                "fast": false,
                "id": "balanced",
                "label": "Balanced",
                "model": "qwen3:1.7b",
                "note": "needs about 2 GB while answering, text only: can't see screenshots",
                "vision": false
              }
            ]
            """;

        var info = LocalAi.Parse(detect, presets);

        Assert.Equal("qwen3:1.7b", info.RecommendedModel);
        Assert.False(info.Vision);
        Assert.Equal("text only: can't see screenshots", info.VisionSummary);
        Assert.Equal("8 GB RAM", info.MachineSummary);
    }

    [Fact]
    public void Handles_insufficient_memory_when_nothing_fits()
    {
        var detect = "ram_mb=1024\ngpu=none\ngpu_discrete=no\nvram_mb=unknown\n";
        var presets = """
            [
              {
                "available": false,
                "budget_mb": 512,
                "download_mb": 0,
                "fast": false,
                "id": "balanced",
                "label": "Balanced",
                "model": null,
                "note": "too little memory for local AI; use a cloud provider",
                "vision": false
              }
            ]
            """;

        var info = LocalAi.Parse(detect, presets);

        Assert.Null(info.RecommendedModel);
        Assert.False(info.HasModel);
        Assert.False(info.Vision);
        Assert.Contains("too little memory for local AI", info.Note);
        Assert.Contains("too little memory for local AI", info.VisionSummary);
    }

    [Fact]
    public void Env_writer_adds_five_keys_and_preserves_existing_settings()
    {
        var existing = "# FRIDAY AI provider credentials.\nOPENAI_API_KEY=sk-test-12345\nFRIDAY_DESKTOP=1\n";
        var updated = LocalAi.WithLocalAi(existing, "gemma3:4b", vision: true, localFirst: false);
        var vars = EnvFile.Parse(updated);

        Assert.Equal("1", vars["LLAMACPP_ENABLED"]);
        Assert.Equal("http://127.0.0.1:11434/v1", vars["LLAMACPP_BASE_URL"]);
        Assert.Equal("gemma3:4b", vars["LLAMACPP_MODEL"]);
        Assert.Equal("1", vars["LLAMACPP_VISION"]);
        Assert.Equal("0", vars["FRIDAY_LOCAL_FIRST"]);

        Assert.Equal("sk-test-12345", vars["OPENAI_API_KEY"]);
        Assert.Equal("1", vars["FRIDAY_DESKTOP"]);
    }

    [Fact]
    public void Env_writer_replaces_old_local_ai_keys()
    {
        var existing = """
            LLAMACPP_ENABLED=0
            LLAMACPP_BASE_URL=http://localhost:8080/v1
            LLAMACPP_MODEL=qwen3:1.7b
            LLAMACPP_VISION=0
            FRIDAY_LOCAL_FIRST=1
            OPENAI_API_KEY=sk-existing
            """;

        var updated = LocalAi.WithLocalAi(existing, "gemma3:12b", vision: true, localFirst: true);
        var vars = EnvFile.Parse(updated);

        Assert.Equal("1", vars["LLAMACPP_ENABLED"]);
        Assert.Equal("http://127.0.0.1:11434/v1", vars["LLAMACPP_BASE_URL"]);
        Assert.Equal("gemma3:12b", vars["LLAMACPP_MODEL"]);
        Assert.Equal("1", vars["LLAMACPP_VISION"]);
        Assert.Equal("1", vars["FRIDAY_LOCAL_FIRST"]);
        Assert.Equal("sk-existing", vars["OPENAI_API_KEY"]);

        // Verify keys appear exactly once in the file
        var lines = updated.Split('\n').Select(l => l.Trim()).Where(l => l.Length > 0).ToList();
        Assert.Single(lines, l => l.StartsWith("LLAMACPP_ENABLED="));
        Assert.Single(lines, l => l.StartsWith("LLAMACPP_MODEL="));
    }

    [Fact]
    public void Turn_off_sets_llamacpp_enabled_to_zero()
    {
        var existing = """
            LLAMACPP_ENABLED=1
            LLAMACPP_MODEL=gemma3:4b
            """;

        var off = LocalAi.TurnOffLocalAi(existing);
        var vars = EnvFile.Parse(off);

        Assert.Equal("0", vars["LLAMACPP_ENABLED"]);
        Assert.Equal("gemma3:4b", vars["LLAMACPP_MODEL"]);
    }

    [Fact]
    public void GetCurrentState_identifies_on_off_and_unconfigured()
    {
        var on = EnvFile.Parse("LLAMACPP_ENABLED=1\nLLAMACPP_MODEL=gemma3:4b");
        var (isOn1, model1) = LocalAi.GetCurrentState(on);
        Assert.True(isOn1);
        Assert.Equal("gemma3:4b", model1);

        var off = EnvFile.Parse("LLAMACPP_ENABLED=0\nLLAMACPP_MODEL=gemma3:4b");
        var (isOn2, model2) = LocalAi.GetCurrentState(off);
        Assert.False(isOn2);
        Assert.Equal("gemma3:4b", model2);

        var unconfigured = EnvFile.Parse("");
        var (isOn3, model3) = LocalAi.GetCurrentState(unconfigured);
        Assert.False(isOn3);
        Assert.Null(model3);
    }
}

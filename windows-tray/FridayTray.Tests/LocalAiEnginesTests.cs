using FridayTray.Core;
using Xunit;

namespace FridayTray.Tests;

public class LocalAiEnginesTests
{
    private const string Json = """
        [{"engine":"ollama","installed":true,"running":true,"base_url":"http://127.0.0.1:11434/v1",
          "models":[{"name":"qwen3:4b","size_mb":2499,"vision":false},{"name":"gemma3:12b","size_mb":7800,"vision":true}]},
         {"engine":"llamacpp","installed":false,"running":false,"base_url":"http://127.0.0.1:8080/v1","models":[]}]
        """;

    [Fact]
    public void Engines_and_their_models_are_read()
    {
        var engines = LocalAi.ParseEngines(Json);
        Assert.Equal(2, engines.Count);
        Assert.True(engines[0].Running);
        Assert.Equal("Ollama (running)", engines[0].Title);
        Assert.Equal(2, engines[0].Models.Count);
        Assert.Equal("gemma3:12b · 7.6 GB · sees screenshots", engines[0].Models[1].Label);
        Assert.False(engines[1].Installed);
    }

    [Theory]
    [InlineData("")]
    [InlineData("not json")]
    [InlineData("{\"engine\":\"ollama\"}")]
    public void Anything_else_is_no_engines(string json) => Assert.Empty(LocalAi.ParseEngines(json));

    [Fact]
    public void Using_a_found_model_points_friday_at_its_engine()
    {
        var env = LocalAi.WithLocalAi("GEMINI_API_KEY=k\n", "gemma3:12b", vision: true, localFirst: true, "http://127.0.0.1:11434/v1");
        Assert.Contains("LLAMACPP_MODEL=gemma3:12b", env);
        Assert.Contains("LLAMACPP_VISION=1", env);
        Assert.Contains("GEMINI_API_KEY=k", env);
    }
}

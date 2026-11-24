using FridayTray.Core;
using Xunit;

public class PickedRegionTests
{
    [Fact]
    public void Corners_in_either_order_give_the_same_area()
    {
        var a = PickedRegion.FromCorners(100, 200, 740, 680);
        var b = PickedRegion.FromCorners(740, 680, 100, 200);
        Assert.Equal(a, b);
        Assert.Equal("100,200 640x480", a!.Value.Arg);
    }

    [Fact]
    public void A_click_or_a_sliver_is_not_an_area()
    {
        Assert.Null(PickedRegion.FromCorners(50, 50, 50, 50));
        Assert.Null(PickedRegion.FromCorners(50, 50, 55, 400));
    }

    [Fact]
    public void A_click_is_a_small_area_around_the_point()
    {
        Assert.True(PickedRegion.IsClick(50, 50, 52, 51));
        Assert.False(PickedRegion.IsClick(50, 50, 55, 400));
        Assert.Equal("396,296 8x8", PickedRegion.Point(400, 300).Arg);
    }

    [Fact]
    public void An_area_left_of_or_above_the_primary_screen_keeps_its_negative_origin()
    {
        Assert.Equal("-1920,-40 800x600", PickedRegion.FromCorners(-1920, -40, -1120, 560)!.Value.Arg);
    }

    [Fact]
    public void The_screen_ask_passes_the_region_only_when_there_is_one()
    {
        var plain = Asking.CtlArguments(AskKind.Screen, "q", "q", null, new AskOptions());
        Assert.DoesNotContain("--region", plain);
        var area = Asking.CtlArguments(AskKind.Screen, "q", "q", null, new AskOptions(Region: "1,2 30x40"));
        Assert.Equal(["--region", "1,2 30x40"], area.TakeLast(2));
        Assert.Contains("--yes", area);
    }
}

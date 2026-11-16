namespace FridayTray.Core;

/// <summary>
/// An area of the desktop the user dragged out, in physical desktop pixels
/// (what the host's <c>--screenshot x y w h</c> and friday-ctl's
/// <c>--region</c> take).
/// </summary>
public readonly record struct PickedRegion(int X, int Y, int Width, int Height)
{
    /// <summary>Smaller than this is a click, not an area.</summary>
    public const int MinSize = 8;

    /// <summary>The area between two corners in either order, or null when it is too small.</summary>
    public static PickedRegion? FromCorners(int x1, int y1, int x2, int y2)
    {
        var region = new PickedRegion(Math.Min(x1, x2), Math.Min(y1, y2), Math.Abs(x2 - x1), Math.Abs(y2 - y1));
        return region.Width < MinSize || region.Height < MinSize ? null : region;
    }

    /// <summary>friday-ctl's <c>--region</c> value: "X,Y WxH".</summary>
    public string Arg => $"{X},{Y} {Width}x{Height}";
}

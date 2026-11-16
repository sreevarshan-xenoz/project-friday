using FridayTray.Core;

namespace FridayTray;

/// <summary>
/// A dim overlay across every monitor: drag out the area to ask about,
/// Escape (or a click without a drag) cancels. Nothing is captured here; the
/// area only says which part of the screen the daemon captures later.
/// </summary>
internal sealed class RegionForm : Form
{
    private Point? _start;
    private Point _now;

    public PickedRegion? Picked { get; private set; }

    public RegionForm()
    {
        FormBorderStyle = FormBorderStyle.None;
        StartPosition = FormStartPosition.Manual;
        Bounds = SystemInformation.VirtualScreen;
        ShowInTaskbar = false;
        TopMost = true;
        KeyPreview = true;
        DoubleBuffered = true;
        BackColor = Color.Black;
        Opacity = 0.35;
        Cursor = Cursors.Cross;
        Shown += (_, _) => Activate();
    }

    // Screen coordinates come from the cursor, not the form, so monitors with
    // different scaling still give the physical pixels the host captures.
    protected override void OnMouseDown(MouseEventArgs e)
    {
        if (e.Button != MouseButtons.Left) return;
        _start = Cursor.Position;
        _now = _start.Value;
    }

    protected override void OnMouseMove(MouseEventArgs e)
    {
        if (_start is null) return;
        _now = Cursor.Position;
        Invalidate();
    }

    protected override void OnMouseUp(MouseEventArgs e)
    {
        if (_start is not { } s) return;
        Picked = PickedRegion.FromCorners(s.X, s.Y, Cursor.Position.X, Cursor.Position.Y);
        DialogResult = Picked is null ? DialogResult.Cancel : DialogResult.OK;
        Close();
    }

    protected override void OnKeyDown(KeyEventArgs e)
    {
        if (e.KeyCode != Keys.Escape) { base.OnKeyDown(e); return; }
        DialogResult = DialogResult.Cancel;
        Close();
        e.Handled = true;
    }

    protected override void OnPaint(PaintEventArgs e)
    {
        if (_start is not { } s) return;
        var a = PointToClient(s);
        var b = PointToClient(_now);
        var rect = Rectangle.FromLTRB(Math.Min(a.X, b.X), Math.Min(a.Y, b.Y), Math.Max(a.X, b.X), Math.Max(a.Y, b.Y));
        using var fill = new SolidBrush(Theme.Accent);
        using var pen = new Pen(Theme.Accent, 2);
        e.Graphics.FillRectangle(fill, rect);
        e.Graphics.DrawRectangle(pen, rect);
    }
}

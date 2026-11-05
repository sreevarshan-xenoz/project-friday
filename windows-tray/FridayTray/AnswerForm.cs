using FridayTray.Core;

namespace FridayTray;

/// <summary>
/// An answer, shown above the tray: the question, the full answer (scrolls),
/// and Copy / Ask again / Close. A failure because FRIDAY isn't running
/// offers Start FRIDAY instead of Ask again.
/// </summary>
internal sealed class AnswerForm : Form
{
    public event Action? AskAgain;
    public event Action? StartFriday;

    public AnswerForm(string question, AskOutcome outcome)
    {
        Text = "FRIDAY";
        FormBorderStyle = FormBorderStyle.None;
        StartPosition = FormStartPosition.Manual;
        ShowInTaskbar = false;
        TopMost = true;
        BackColor = Theme.Bg;
        Size = new Size(460, 320);
        Padding = new Padding(16, 12, 16, 12);

        var title = new Label
        {
            Text = (outcome.Ok ? "●  " : "✕  ") + question,
            Dock = DockStyle.Top,
            Height = 28,
            Font = Theme.Title,
            ForeColor = outcome.Ok ? Theme.Accent : Theme.Error,
            AutoEllipsis = true,
        };
        var body = new TextBox
        {
            Text = outcome.Answer.Replace("\r\n", "\n").Replace("\n", Environment.NewLine),
            Multiline = true,
            ReadOnly = true,
            ScrollBars = ScrollBars.Vertical,
            BorderStyle = BorderStyle.None,
            BackColor = Theme.Bg,
            ForeColor = Theme.Text,
            Font = Theme.Body,
            Dock = DockStyle.Fill,
        };
        var buttons = new FlowLayoutPanel
        {
            Dock = DockStyle.Bottom,
            Height = 38,
            FlowDirection = FlowDirection.RightToLeft,
            BackColor = Theme.Bg,
            Padding = new Padding(0, 6, 0, 0),
        };
        var close = Theme.Button("Close");
        close.Click += (_, _) => Close();
        var copy = Theme.Button("Copy", primary: outcome.Ok);
        copy.Click += (_, _) => { Clipboard.SetText(outcome.Answer); copy.Text = "Copied"; };
        buttons.Controls.Add(close);
        if (outcome.DaemonDown)
        {
            var start = Theme.Button("Start FRIDAY", primary: true);
            start.Click += (_, _) => { StartFriday?.Invoke(); Close(); };
            buttons.Controls.Add(start);
        }
        else
        {
            var again = Theme.Button("Ask again");
            again.Click += (_, _) => { AskAgain?.Invoke(); Close(); };
            buttons.Controls.Add(again);
            buttons.Controls.Add(copy);
        }

        Controls.Add(body);
        Controls.Add(buttons);
        Controls.Add(title);

        var area = Screen.PrimaryScreen?.WorkingArea ?? new Rectangle(0, 0, 1280, 800);
        Location = new Point(area.Right - Width - 16, area.Bottom - Height - 16);
        Paint += (_, e) => ControlPaint.DrawBorder(e.Graphics, ClientRectangle,
            outcome.Ok ? Theme.Outline : Theme.Error, ButtonBorderStyle.Solid);
        KeyPreview = true;
        KeyDown += (_, e) => { if (e.KeyCode == Keys.Escape) Close(); };
    }
}

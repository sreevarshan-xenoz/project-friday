using FridayTray.Core;

namespace FridayTray;

/// <summary>
/// An answer, shown above the tray: the question, the full answer (scrolls),
/// and Copy / Ask again / Close. It opens as soon as the question is asked,
/// fills in while the answer streams (<see cref="ShowPartial"/>), then shows
/// the outcome (<see cref="ShowOutcome"/>). A failure because FRIDAY isn't
/// running offers Start FRIDAY instead of Ask again.
/// </summary>
internal sealed class AnswerForm : Form
{
    public event Action? AskAgain;
    public event Action? StartFriday;
    public event Action<string>? ConfirmRun;

    private readonly string _question;
    private readonly Label _title;
    private readonly TextBox _body;
    private readonly FlowLayoutPanel _buttons;
    private Color _border = Theme.Outline;

    /// <summary>A window for an answer that is still on its way.</summary>
    public AnswerForm(string question, string waiting = "Thinking…")
    {
        _question = question;
        Text = "FRIDAY";
        FormBorderStyle = FormBorderStyle.None;
        StartPosition = FormStartPosition.Manual;
        ShowInTaskbar = false;
        TopMost = true;
        BackColor = Theme.Bg;
        Size = new Size(460, 320);
        Padding = new Padding(16, 12, 16, 12);

        _title = new Label
        {
            Text = "…  " + question,
            Dock = DockStyle.Top,
            Height = 28,
            Font = Theme.Title,
            ForeColor = Theme.Accent,
            AutoEllipsis = true,
        };
        _body = new TextBox
        {
            Text = waiting,
            Multiline = true,
            ReadOnly = true,
            ScrollBars = ScrollBars.Vertical,
            BorderStyle = BorderStyle.None,
            BackColor = Theme.Bg,
            ForeColor = Theme.Subtext,
            Font = Theme.Body,
            Dock = DockStyle.Fill,
        };
        _buttons = new FlowLayoutPanel
        {
            Dock = DockStyle.Bottom,
            Height = 38,
            FlowDirection = FlowDirection.RightToLeft,
            BackColor = Theme.Bg,
            Padding = new Padding(0, 6, 0, 0),
        };
        _buttons.Controls.Add(CloseButton());

        Controls.Add(_body);
        Controls.Add(_buttons);
        Controls.Add(_title);

        var area = Screen.PrimaryScreen?.WorkingArea ?? new Rectangle(0, 0, 1280, 800);
        Location = new Point(area.Right - Width - 16, area.Bottom - Height - 16);
        Paint += (_, e) => ControlPaint.DrawBorder(e.Graphics, ClientRectangle, _border, ButtonBorderStyle.Solid);
        KeyPreview = true;
        KeyDown += (_, e) => { if (e.KeyCode == Keys.Escape) Close(); };
    }

    /// <summary>A window showing a finished answer.</summary>
    public AnswerForm(string question, AskOutcome outcome) : this(question) => ShowOutcome(outcome);

    /// <summary>Reset the form back to waiting state (e.g. when confirming and re-running).</summary>
    public void ShowWaiting(string waiting = "Thinking…")
    {
        _title.Text = "…  " + _question;
        _title.ForeColor = Theme.Accent;
        _body.Font = Theme.Body;
        _body.ForeColor = Theme.Subtext;
        _body.Text = waiting;
        _border = Theme.Outline;
        _buttons.Controls.Clear();
        var close = CloseButton();
        _buttons.Controls.Add(close);
        AcceptButton = null;
        CancelButton = close;
        Invalidate();
    }

    /// <summary>The answer so far, while it streams; the newest text stays in view.</summary>
    public void ShowPartial(string text)
    {
        _body.Font = Theme.Body;
        _body.ForeColor = Theme.Text;
        _body.Text = Lines(text);
        _body.SelectionStart = _body.TextLength;
        _body.ScrollToCaret();
    }

    /// <summary>The final answer, confirmation prompt, or failure, with its buttons.</summary>
    public void ShowOutcome(AskOutcome outcome)
    {
        _buttons.Controls.Clear();
        AcceptButton = null;
        CancelButton = null;

        if (outcome.Confirmation is { } conf)
        {
            _title.Text = "⚠  " + _question;
            _title.ForeColor = Theme.Warn;
            _body.Font = Theme.Monospace;
            _body.ForeColor = Theme.Text;
            _body.Text = Lines(outcome.Answer);
            _body.SelectionStart = 0;
            _body.ScrollToCaret();
            _border = Theme.Warn;

            var cancel = Theme.Button("Cancel");
            cancel.Click += (_, _) => Close();

            var run = Theme.Button("Run", primary: true);
            run.Click += (_, _) => ConfirmRun?.Invoke(conf.Action);

            // FlowDirection is RightToLeft: adding cancel first puts it on the right,
            // adding run second puts it to the left of cancel: [Run] [Cancel].
            _buttons.Controls.Add(cancel);
            _buttons.Controls.Add(run);

            // Cancel is the default / Escape
            CancelButton = cancel;
            AcceptButton = cancel;
            cancel.Select();

            Invalidate();
            return;
        }

        _body.Font = Theme.Body;
        _title.Text = (outcome.Ok ? "●  " : "✕  ") + _question;
        _title.ForeColor = outcome.Ok ? Theme.Accent : Theme.Error;
        _body.ForeColor = Theme.Text;
        _body.Text = Lines(outcome.Answer);
        _body.SelectionStart = 0;
        _body.ScrollToCaret();
        _border = outcome.Ok ? Theme.Outline : Theme.Error;

        var close = CloseButton();
        _buttons.Controls.Add(close);
        CancelButton = close;
        if (outcome.DaemonDown)
        {
            var start = Theme.Button("Start FRIDAY", primary: true);
            start.Click += (_, _) => { StartFriday?.Invoke(); Close(); };
            _buttons.Controls.Add(start);
        }
        else
        {
            var again = Theme.Button("Ask again");
            again.Click += (_, _) => { AskAgain?.Invoke(); Close(); };
            _buttons.Controls.Add(again);
            var copy = Theme.Button("Copy", primary: outcome.Ok);
            copy.Click += (_, _) => { Clipboard.SetText(outcome.Answer); copy.Text = "Copied"; };
            _buttons.Controls.Add(copy);
        }
        Invalidate();
    }

    private Button CloseButton()
    {
        var close = Theme.Button("Close");
        close.Click += (_, _) => Close();
        return close;
    }

    private static string Lines(string text) =>
        text.Replace("\r\n", "\n").Replace("\n", Environment.NewLine);
}

using FridayTray.Core;

namespace FridayTray;

/// <summary>
/// The ask box: a slim dark bar near the top of the screen the mouse is on.
/// Enter asks, Escape or clicking away closes, Up/Down walks the history.
/// For selection and screen asks a line above says what goes with it; with a
/// selection, the quick actions are listed under the box (click one, or walk
/// them with Up/Down; what you typed instead is sent as typed).
/// </summary>
internal sealed class AskForm : Form
{
    private readonly TextBox _input;
    private readonly IReadOnlyList<string> _history;
    private int _historyIndex = -1;
    private readonly ListBox? _quick;

    public string Question => _input.Text.Trim();

    public AskForm(AskKind kind, string? contextPreview, IReadOnlyList<string> history)
    {
        _history = history;
        Text = "Ask FRIDAY";
        FormBorderStyle = FormBorderStyle.None;
        StartPosition = FormStartPosition.Manual;
        ShowInTaskbar = false;
        TopMost = true;
        KeyPreview = true;
        BackColor = Theme.Surface;
        Padding = new Padding(2);
        Width = 720;

        var hint = kind switch
        {
            AskKind.Selection => $"About: “{contextPreview}…”   (pick one below or type your own)",
            AskKind.Screen => "About your screen: a screenshot goes to your AI provider, this once.",
            _ => "Ask anything · Up/Down for recent questions",
        };

        var panel = new Panel { Dock = DockStyle.Fill, BackColor = Theme.Bg, Padding = new Padding(18, 10, 18, 12) };
        var label = new Label
        {
            Text = "FRIDAY  ›  " + hint,
            Dock = DockStyle.Top,
            Height = 22,
            ForeColor = kind == AskKind.Screen ? Theme.Warn : Theme.Subtext,
            Font = Theme.Small,
            AutoEllipsis = true,
        };
        _input = new TextBox
        {
            Dock = DockStyle.Top,
            BorderStyle = BorderStyle.None,
            BackColor = Theme.Bg,
            ForeColor = Theme.Text,
            Font = Theme.Input,
        };
        var extra = 0;
        if (kind == AskKind.Selection && history.Count > 0)
        {
            _quick = new ListBox
            {
                Dock = DockStyle.Top,
                BorderStyle = BorderStyle.None,
                BackColor = Theme.Bg,
                ForeColor = Theme.Subtext,
                Font = Theme.Small,
                IntegralHeight = false,
                Height = Math.Min(history.Count, 8) * 20 + 6,
            };
            foreach (var action in history)
                _quick.Items.Add(action);
            // A click picks one and asks it straight away.
            _quick.MouseUp += (_, e) =>
            {
                var i = _quick.IndexFromPoint(e.Location);
                if (i < 0) return;
                SetText(history[i]);
                DialogResult = DialogResult.OK;
                Close();
            };
            extra = _quick.Height + 6;
            panel.Controls.Add(_quick);
        }
        panel.Controls.Add(_input);
        panel.Controls.Add(label);
        Controls.Add(panel);
        Height = 22 + _input.PreferredHeight + 26 + extra;

        var screen = Screen.FromPoint(Cursor.Position).WorkingArea;
        Location = new Point(screen.Left + (screen.Width - Width) / 2, screen.Top + screen.Height / 4);
        Paint += (_, e) => ControlPaint.DrawBorder(e.Graphics, ClientRectangle, Theme.Accent, ButtonBorderStyle.Solid);
        // Clicking away closes it, but only once it has had focus: Windows may
        // refuse a background app the foreground, and an ask box that vanishes
        // the moment it opens would be worse than one that stays.
        var wasActive = false;
        Activated += (_, _) => wasActive = true;
        Deactivate += (_, _) =>
        {
            if (wasActive && DialogResult == DialogResult.None)
            {
                DialogResult = DialogResult.Cancel;
                Close();
            }
        };
        Shown += (_, _) => { Activate(); _input.Focus(); };
    }

    protected override void OnKeyDown(KeyEventArgs e)
    {
        switch (e.KeyCode)
        {
            case Keys.Escape:
                DialogResult = DialogResult.Cancel;
                Close();
                break;
            case Keys.Enter when Question.Length > 0:
                DialogResult = DialogResult.OK;
                Close();
                break;
            case Keys.Up when _history.Count > 0:
                _historyIndex = Math.Min(_history.Count - 1, _historyIndex + 1);
                SetText(_history[_historyIndex]);
                if (_quick != null) _quick.SelectedIndex = _historyIndex;
                break;
            case Keys.Down when _historyIndex > 0:
                _historyIndex--;
                SetText(_history[_historyIndex]);
                if (_quick != null) _quick.SelectedIndex = _historyIndex;
                break;
            default:
                base.OnKeyDown(e);
                return;
        }
        e.Handled = e.SuppressKeyPress = true;
    }

    private void SetText(string text)
    {
        _input.Text = text;
        _input.SelectionStart = text.Length;
    }
}

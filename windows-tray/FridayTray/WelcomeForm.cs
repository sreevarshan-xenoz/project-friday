using FridayTray.Core;

namespace FridayTray;

/// <summary>
/// First run: what FRIDAY is, the keys, and the two choices worth making now
/// (which AI, and whether to look at Settings). Everything else stays off.
/// </summary>
internal sealed class WelcomeForm : Form
{
    public WelcomeForm(IReadOnlyDictionary<AskKind, string> keys, Action chooseProvider, Action openSettings, Action tryIt)
    {
        Text = "Welcome to FRIDAY";
        FormBorderStyle = FormBorderStyle.FixedDialog;
        MaximizeBox = MinimizeBox = false;
        StartPosition = FormStartPosition.CenterScreen;
        BackColor = Theme.Bg;
        ForeColor = Theme.Text;
        Font = Theme.Body;
        ClientSize = new Size(500, 420);
        Padding = new Padding(22);

        var layout = new FlowLayoutPanel
        {
            Dock = DockStyle.Fill,
            FlowDirection = FlowDirection.TopDown,
            WrapContents = false,
            BackColor = Theme.Bg,
        };

        layout.Controls.Add(new Label { Text = "Hello. I'm FRIDAY.", Font = new Font("Segoe UI Semibold", 18f), AutoSize = true, ForeColor = Theme.Accent });
        layout.Controls.Add(Paragraph("Ask me about what's on your screen or what you've selected, and I'll answer or do it. I only act when you ask.", Theme.Subtext, 0, 6));

        string Key(AskKind k, string fallback) => keys.TryGetValue(k, out var l) ? l : fallback;
        layout.Controls.Add(KeyRow(Key(AskKind.Plain, "tray icon"), "ask anything"));
        layout.Controls.Add(KeyRow(Key(AskKind.Selection, "(taken)"), "ask about the text you selected"));
        layout.Controls.Add(KeyRow(Key(AskKind.Screen, "(taken)"), "ask about your screen (a screenshot goes to your AI provider, this once)"));

        layout.Controls.Add(Paragraph("Until you add your own key, I use free models. They may keep what you send them, so for anything private choose your own provider.", Theme.Subtext, 14, 0));

        var buttons = new FlowLayoutPanel { AutoSize = true, WrapContents = false, BackColor = Theme.Bg, Margin = new Padding(0, 16, 0, 0) };
        var provider = Theme.Button("Choose my AI…");
        provider.Click += (_, _) => chooseProvider();
        var settings = Theme.Button("Settings…");
        settings.Click += (_, _) => openSettings();
        var tryNow = Theme.Button("Try it now", primary: true);
        tryNow.Click += (_, _) => { Close(); tryIt(); };
        buttons.Controls.AddRange(new Control[] { provider, settings, tryNow });
        layout.Controls.Add(buttons);

        var done = Theme.Button("Done");
        done.Click += (_, _) => Close();
        layout.Controls.Add(done);
        AcceptButton = tryNow;
        CancelButton = done;
        Controls.Add(layout);
    }

    private static Label Paragraph(string text, Color color, int top, int bottom) => new()
    {
        Text = text,
        AutoSize = true,
        MaximumSize = new Size(456, 0),
        ForeColor = color,
        Margin = new Padding(0, top, 0, bottom),
    };

    private static Control KeyRow(string key, string what)
    {
        var row = new FlowLayoutPanel { AutoSize = true, WrapContents = false, BackColor = Theme.Bg, Margin = new Padding(0, 6, 0, 0) };
        row.Controls.Add(new Label
        {
            Text = key,
            AutoSize = false,
            Width = 110,
            Height = 26,
            TextAlign = ContentAlignment.MiddleCenter,
            BackColor = Theme.Raised,
            ForeColor = Theme.Text,
            Font = Theme.Small,
            Margin = new Padding(0, 0, 10, 0),
        });
        row.Controls.Add(new Label { Text = what, AutoSize = true, MaximumSize = new Size(330, 0), ForeColor = Theme.Text, Margin = new Padding(0, 4, 0, 0) });
        return row;
    }
}

using System.Windows;
using System.Windows.Media;
using System.Windows.Media.Animation;

namespace FridayTray.Fluent;

/// <summary>
/// A damped spring: moves past the target a little and settles, the way a
/// real thing does when let go. Lower <see cref="Damping"/> bounces more;
/// 1 settles without overshoot.
/// </summary>
public sealed class Spring : EasingFunctionBase
{
    public static readonly DependencyProperty DampingProperty =
        DependencyProperty.Register(nameof(Damping), typeof(double), typeof(Spring), new PropertyMetadata(0.62));

    /// <summary>How many swings fit in the animation's duration.</summary>
    public static readonly DependencyProperty FrequencyProperty =
        DependencyProperty.Register(nameof(Frequency), typeof(double), typeof(Spring), new PropertyMetadata(1.5));

    public Spring() => EasingMode = EasingMode.EaseIn; // EaseInCore is the whole curve

    public double Damping
    {
        get => (double)GetValue(DampingProperty);
        set => SetValue(DampingProperty, value);
    }

    public double Frequency
    {
        get => (double)GetValue(FrequencyProperty);
        set => SetValue(FrequencyProperty, value);
    }

    protected override double EaseInCore(double t)
    {
        if (t >= 1)
            return 1;
        var w = 2 * Math.PI * Frequency;
        var z = Math.Clamp(Damping, 0.05, 0.999);
        var wd = w * Math.Sqrt(1 - z * z);
        return 1 - Math.Exp(-z * w * t) * (Math.Cos(wd * t) + z * w / wd * Math.Sin(wd * t));
    }

    protected override Freezable CreateInstanceCore() => new Spring();
}

/// <summary>The few movements every window shares, so they all feel the same.</summary>
internal static class Motion
{
    public static readonly IEasingFunction Out = new CubicEase { EasingMode = EasingMode.EaseOut };
    public static readonly IEasingFunction In = new CubicEase { EasingMode = EasingMode.EaseIn };

    /// <summary>Fades and slides <paramref name="e"/> into place from <paramref name="dy"/> below, after <paramref name="delayMs"/>.</summary>
    public static void RiseIn(UIElement e, double dy = 14, int delayMs = 0, int ms = 520, double fromScale = 1)
    {
        var move = new TranslateTransform(0, dy);
        var scale = new ScaleTransform(fromScale, fromScale);
        e.RenderTransformOrigin = new System.Windows.Point(0.5, 1);
        e.RenderTransform = new TransformGroup { Children = { scale, move } };
        e.Opacity = 0;
        var begin = TimeSpan.FromMilliseconds(delayMs);
        var spring = new Spring { Damping = 0.72 };
        e.BeginAnimation(UIElement.OpacityProperty,
            new DoubleAnimation(1, TimeSpan.FromMilliseconds(Math.Min(ms, 240))) { BeginTime = begin, EasingFunction = Out });
        move.BeginAnimation(TranslateTransform.YProperty,
            new DoubleAnimation(0, TimeSpan.FromMilliseconds(ms)) { BeginTime = begin, EasingFunction = spring });
        if (fromScale != 1)
        {
            scale.BeginAnimation(ScaleTransform.ScaleXProperty, new DoubleAnimation(1, TimeSpan.FromMilliseconds(ms)) { BeginTime = begin, EasingFunction = spring });
            scale.BeginAnimation(ScaleTransform.ScaleYProperty, new DoubleAnimation(1, TimeSpan.FromMilliseconds(ms)) { BeginTime = begin, EasingFunction = spring });
        }
    }

    /// <summary>Fades <paramref name="e"/> out while it sinks a little, then runs <paramref name="done"/>.</summary>
    public static void SinkOut(UIElement e, Action done, int ms = 150)
    {
        var move = new TranslateTransform();
        e.RenderTransform = move;
        var fade = new DoubleAnimation(0, TimeSpan.FromMilliseconds(ms)) { EasingFunction = In };
        fade.Completed += (_, _) => done();
        move.BeginAnimation(TranslateTransform.YProperty, new DoubleAnimation(8, TimeSpan.FromMilliseconds(ms)) { EasingFunction = In });
        e.BeginAnimation(UIElement.OpacityProperty, fade);
    }

    /// <summary>A gentle cross-fade for content that changes in place.</summary>
    public static void FadeIn(UIElement e, int ms = 200) =>
        e.BeginAnimation(UIElement.OpacityProperty, new DoubleAnimation(0, 1, TimeSpan.FromMilliseconds(ms)) { EasingFunction = Out });
}

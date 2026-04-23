namespace findamodel.Data.Entities;

public class PrinterConfig
{
    public const float DefaultLayerHeightMm = 0.05f;
    public const int DefaultBottomLayerCount = 4;
    public const int DefaultTransitionLayerCount = 0;
    public const float DefaultExposureTimeSeconds = 2.5f;
    public const float DefaultBottomExposureTimeSeconds = 30f;
    public const float DefaultBottomLiftHeightMm = 6f;
    public const float DefaultBottomLiftSpeedMmPerMinute = 65f;
    public const float DefaultLiftHeightMm = 6f;
    public const float DefaultLiftSpeedMmPerMinute = 80f;
    public const float DefaultRetractSpeedMmPerMinute = 150f;
    public const float DefaultBottomLightOffDelaySeconds = 0f;
    public const float DefaultLightOffDelaySeconds = 0f;
    public const float DefaultWaitTimeBeforeCureSeconds = 0f;
    public const float DefaultWaitTimeAfterCureSeconds = 0f;
    public const float DefaultWaitTimeAfterLiftSeconds = 0f;
    public const byte DefaultLightPwm = byte.MaxValue;
    public const byte DefaultBottomLightPwm = byte.MaxValue;

    public Guid Id { get; set; }
    public string Name { get; set; } = "";
    public float BedWidthMm { get; set; }
    public float BedDepthMm { get; set; }
    public int PixelWidth { get; set; }
    public int PixelHeight { get; set; }
    public float LayerHeightMm { get; set; } = DefaultLayerHeightMm;
    public int BottomLayerCount { get; set; } = DefaultBottomLayerCount;
    public int TransitionLayerCount { get; set; } = DefaultTransitionLayerCount;
    public float ExposureTimeSeconds { get; set; } = DefaultExposureTimeSeconds;
    public float BottomExposureTimeSeconds { get; set; } = DefaultBottomExposureTimeSeconds;
    public float BottomLiftHeightMm { get; set; } = DefaultBottomLiftHeightMm;
    public float BottomLiftSpeedMmPerMinute { get; set; } = DefaultBottomLiftSpeedMmPerMinute;
    public float LiftHeightMm { get; set; } = DefaultLiftHeightMm;
    public float LiftSpeedMmPerMinute { get; set; } = DefaultLiftSpeedMmPerMinute;
    public float RetractSpeedMmPerMinute { get; set; } = DefaultRetractSpeedMmPerMinute;
    public float BottomLightOffDelaySeconds { get; set; } = DefaultBottomLightOffDelaySeconds;
    public float LightOffDelaySeconds { get; set; } = DefaultLightOffDelaySeconds;
    public float WaitTimeBeforeCureSeconds { get; set; } = DefaultWaitTimeBeforeCureSeconds;
    public float WaitTimeAfterCureSeconds { get; set; } = DefaultWaitTimeAfterCureSeconds;
    public float WaitTimeAfterLiftSeconds { get; set; } = DefaultWaitTimeAfterLiftSeconds;
    public byte LightPwm { get; set; } = DefaultLightPwm;
    public byte BottomLightPwm { get; set; } = DefaultBottomLightPwm;
    public bool IsBuiltIn { get; set; }
    public bool IsDefault { get; set; }

    // Navigation: printing lists using this printer
    public List<PrintingList> PrintingLists { get; set; } = [];
}

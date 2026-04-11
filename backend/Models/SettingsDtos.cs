namespace findamodel.Models;

public record AppConfigDto(float DefaultRaftHeightMm, string Theme);

public record UpdateAppConfigRequest(float DefaultRaftHeightMm, string Theme);

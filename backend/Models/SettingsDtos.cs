namespace findamodel.Models;

public record AppConfigDto(float DefaultRaftHeightMm);

public record UpdateAppConfigRequest(float DefaultRaftHeightMm);

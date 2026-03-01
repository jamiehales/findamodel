using System.Security.Claims;
using findamodel.Services;
using Microsoft.AspNetCore.Mvc;

namespace findamodel.Controllers;

[ApiController]
[Route("api/[controller]")]
public class UsersController(UserService userService) : ControllerBase
{
    [HttpGet("me")]
    public async Task<IActionResult> GetCurrentUser()
    {
        var userIdStr = User.FindFirstValue(ClaimTypes.NameIdentifier);
        if (userIdStr == null || !Guid.TryParse(userIdStr, out var userId))
            return Unauthorized();

        var user = await userService.GetUserByIdAsync(userId);
        if (user == null) return NotFound();

        return Ok(new { user.Id, user.Username, user.IsAdmin });
    }
}

# Method 4 - External slicer integration

## Overview
Delegate slicing to a mature external tool such as a vendor slicer or CLI-capable engine, then import the resulting images or packaged build file.

## Pros
- Fastest way to reach production-grade slicing features
- Mature support generation, hollowing, exposure tuning, and machine profiles
- Lower algorithmic risk inside this codebase

## Cons
- Adds a dependency on third-party tooling
- Harder to make fully portable and self-contained
- Integration contracts can break across slicer versions

## Expected speed
- High feature velocity for development
- Runtime speed depends on the external engine

## Implementation complexity
Medium for integration, low for slicing logic inside this app.

## Concrete implementation plan
1. Export the arranged plate mesh to an interchange format such as 3MF or STL.
2. Invoke the selected slicer with a machine profile.
3. Capture its output archive or layer images.
4. Surface progress, logs, and errors back through the existing job API.
5. Cache completed jobs by mesh checksum and printer profile.

## Best use case
When full print preparation, not just raw slice masks, becomes a product priority.

Implement a second support autogeneration algorithm, and add it as "generate autosupport (method 2)" and rename the other to method 1.
Generate a voxel/octree or similar data structure for storing a representation of the model - use 2mm as the area for now. Note I will say voxel throughout this, but this is just for explanation, use whatever data represenation will be fastest to implement this algorithm.
Add a support at the bottom-most level, and calculate the max amount of force that will be generated on it - force is calculated as all connected voxels of that island (each connected voxel at each layer filled vertically), note that there's a force generated for each layer, based on the peel force which is the contact area of the contact area of that voxel island
Calculate the support positions on a voxel basis for now, add them at the center point of the bottom of the voxel
As the force that will be generated on the support exceeds the max force allowed on the support based on the area of the support, add a new support.
The new support should be placed in the area which will provide the most amount of reduction in the pull force on the supports with the max current pull force
There should also be a calculation of a rotational force, and a max value for that too before new supports are placed
If there is a new unsupported island of a minimum size detected, add a support at that location
All forces should be calculated based on all the supports currently placed on that model at each iteration
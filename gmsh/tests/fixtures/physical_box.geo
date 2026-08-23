SetFactory("OpenCASCADE");

Box(1) = {0, 0, 0, 1, 1, 1};

Physical Surface("fixed face", 11) = {1};
Physical Volume("steel solid", 21) = {1};

Mesh.MeshSizeMin = 0.75;
Mesh.MeshSizeMax = 0.75;
Mesh.MshFileVersion = 4.1;
Mesh.Binary = 0;

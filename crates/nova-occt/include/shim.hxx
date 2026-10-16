// C++ side of the nova-occt bridge. Every function catches OCCT's own
// exceptions (Standard_Failure, which is not a std::exception) and rethrows
// them as std::runtime_error, which cxx turns into a Rust `Err`.
#pragma once

#include "rust/cxx.h"
#include <TopoDS_Shape.hxx>
#include <memory>

namespace nova {

struct P3;
struct Seg;
struct MeshOut;
struct EdgeData;
struct FaceGeom;
struct ProjEdge;
struct Bounds;

class Shape {
public:
  TopoDS_Shape s;
};

std::unique_ptr<Shape> planar_face(rust::Slice<const Seg> segs, rust::Slice<const uint32_t> loop_lens);
std::unique_ptr<Shape> prism(const Shape &face, P3 dir);
std::unique_ptr<Shape> revolve(const Shape &face, P3 origin, P3 axis, double angle);
std::unique_ptr<Shape> fuse(const Shape &a, const Shape &b);
std::unique_ptr<Shape> cut(const Shape &a, const Shape &b);
std::unique_ptr<Shape> fillet(const Shape &s, rust::Slice<const uint32_t> edges, double radius);
std::unique_ptr<Shape> chamfer(const Shape &s, rust::Slice<const uint32_t> edges, double distance);
MeshOut triangulate(const Shape &s, double tolerance);
double volume(const Shape &s);
Bounds bounds(const Shape &s);
size_t face_count(const Shape &s);
size_t solid_count(const Shape &s);
rust::Vec<EdgeData> edges(const Shape &s);
rust::Vec<FaceGeom> face_geometry(const Shape &s);
std::unique_ptr<Shape> transformed(const Shape &s, rust::Slice<const double> m);
std::unique_ptr<Shape> common(const Shape &a, const Shape &b);
std::unique_ptr<Shape> compound_with(const Shape &c, const Shape &s);
void write_step(const Shape &s, rust::Str path);
rust::Vec<ProjEdge> hlr_project(const Shape &s, P3 dir, P3 x_dir, bool hidden, double deflection);
std::unique_ptr<Shape> clone_shape(const Shape &s);
bool is_valid(const Shape &s);

} // namespace nova

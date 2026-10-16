#include "nova-occt/include/shim.hxx"
#include "nova-occt/src/lib.rs.h"

#include <BRepAdaptor_Curve.hxx>
#include <BRepAdaptor_Surface.hxx>
#include <BRepAlgoAPI_Common.hxx>
#include <BRepAlgoAPI_Cut.hxx>
#include <BRepAlgoAPI_Fuse.hxx>
#include <BRepBndLib.hxx>
#include <BRepBuilderAPI_MakeEdge.hxx>
#include <BRepBuilderAPI_MakeFace.hxx>
#include <BRepBuilderAPI_MakeWire.hxx>
#include <BRepBuilderAPI_Transform.hxx>
#include <BRep_Builder.hxx>
#include <BRepCheck_Analyzer.hxx>
#include <BRepFilletAPI_MakeChamfer.hxx>
#include <BRepFilletAPI_MakeFillet.hxx>
#include <BRepGProp.hxx>
#include <BRepLib_ToolTriangulatedShape.hxx>
#include <BRepMesh_IncrementalMesh.hxx>
#include <BRepPrimAPI_MakePrism.hxx>
#include <BRepPrimAPI_MakeRevol.hxx>
#include <BRepTools.hxx>
#include <BRep_Tool.hxx>
#include <Bnd_Box.hxx>
#include <GCPnts_QuasiUniformDeflection.hxx>
#include <GC_MakeArcOfCircle.hxx>
#include <GProp_GProps.hxx>
#include <Interface_Static.hxx>
#include <Message.hxx>
#include <Message_Messenger.hxx>
#include <Poly_Triangulation.hxx>
#include <STEPControl_Writer.hxx>
#include <ShapeFix_Face.hxx>
#include <ShapeUpgrade_UnifySameDomain.hxx>
#include <Standard_Failure.hxx>
#include <TopExp.hxx>
#include <TopExp_Explorer.hxx>
#include <TopTools_IndexedMapOfShape.hxx>
#include <TopoDS.hxx>
#include <TopoDS_Compound.hxx>
#include <TopoDS_Edge.hxx>
#include <TopoDS_Iterator.hxx>
#include <TopoDS_Face.hxx>
#include <TopoDS_Wire.hxx>
#include <gp_Ax1.hxx>
#include <gp_Cylinder.hxx>
#include <gp_Trsf.hxx>
#include <gp_Pln.hxx>

#include <stdexcept>
#include <string>
#include <vector>

namespace nova {

namespace {

// Run `f`, turning every OCCT failure into a std::runtime_error
template <typename F> auto guard(const char *what, F f) -> decltype(f()) {
  try {
    return f();
  } catch (Standard_Failure const &e) {
    std::string msg = what;
    msg += " failed (";
    msg += e.DynamicType()->Name();
    const char *detail = e.GetMessageString();
    if (detail != nullptr && detail[0] != '\0') {
      msg += ": ";
      msg += detail;
    }
    msg += ")";
    throw std::runtime_error(msg);
  } catch (std::exception const &e) {
    throw std::runtime_error(std::string(what) + " failed: " + e.what());
  } catch (...) {
    throw std::runtime_error(std::string(what) + " failed (unknown OCCT error)");
  }
}

gp_Pnt pnt(const P3 &p) { return gp_Pnt(p.x, p.y, p.z); }
P3 p3(const gp_Pnt &p) { return P3{p.X(), p.Y(), p.Z()}; }
P3 p3(const gp_Dir &d) { return P3{d.X(), d.Y(), d.Z()}; }

std::unique_ptr<Shape> wrap(const TopoDS_Shape &s, const char *what) {
  if (s.IsNull()) {
    throw std::runtime_error(std::string(what) + " produced an empty shape");
  }
  auto out = std::make_unique<Shape>();
  out->s = s;
  return out;
}

size_t count(const TopoDS_Shape &s, TopAbs_ShapeEnum kind) {
  TopTools_IndexedMapOfShape map;
  TopExp::MapShapes(s, kind, map);
  return static_cast<size_t>(map.Extent());
}

TopoDS_Shape unify(const TopoDS_Shape &s) {
  // Merge coplanar/cylindrical pieces split by booleans
  ShapeUpgrade_UnifySameDomain unifier(s, Standard_True, Standard_True, Standard_True);
  unifier.Build();
  return unifier.Shape();
}

TopoDS_Shape checked_solid_result(const TopoDS_Shape &s, const char *what) {
  if (s.IsNull() || count(s, TopAbs_SOLID) == 0) {
    throw std::runtime_error(std::string(what) + " left no material");
  }
  BRepCheck_Analyzer check(s);
  if (!check.IsValid()) {
    throw std::runtime_error(std::string(what) + " produced an invalid solid");
  }
  return s;
}

} // namespace

std::unique_ptr<Shape> planar_face(rust::Slice<const Seg> segs, rust::Slice<const uint32_t> loop_lens) {
  return guard("profile face", [&]() {
    std::vector<TopoDS_Wire> wires;
    size_t i = 0;
    for (uint32_t len : loop_lens) {
      BRepBuilderAPI_MakeWire mw;
      for (uint32_t k = 0; k < len; ++k, ++i) {
        if (i >= segs.size()) {
          throw std::runtime_error("loop lengths exceed segments");
        }
        const Seg &sg = segs[i];
        TopoDS_Edge edge;
        if (sg.kind == 0) {
          edge = BRepBuilderAPI_MakeEdge(pnt(sg.a), pnt(sg.b)).Edge();
        } else {
          GC_MakeArcOfCircle arc(pnt(sg.a), pnt(sg.c), pnt(sg.b));
          if (!arc.IsDone()) {
            throw std::runtime_error("degenerate arc");
          }
          edge = BRepBuilderAPI_MakeEdge(arc.Value()).Edge();
        }
        mw.Add(edge);
        if (!mw.IsDone()) {
          throw std::runtime_error("profile loop is not connected");
        }
      }
      wires.push_back(mw.Wire());
    }
    if (wires.empty()) {
      throw std::runtime_error("no profile loop");
    }
    BRepBuilderAPI_MakeFace mf(wires[0], Standard_True);
    if (!mf.IsDone()) {
      throw std::runtime_error("profile is not planar");
    }
    for (size_t w = 1; w < wires.size(); ++w) {
      mf.Add(wires[w]);
    }
    ShapeFix_Face fix(mf.Face());
    fix.FixOrientation();
    fix.Perform();
    return wrap(fix.Face(), "profile face");
  });
}

std::unique_ptr<Shape> prism(const Shape &face, P3 dir) {
  return guard("extrusion", [&]() {
    BRepPrimAPI_MakePrism mk(face.s, gp_Vec(dir.x, dir.y, dir.z));
    mk.Build();
    if (!mk.IsDone()) {
      throw std::runtime_error("not done");
    }
    return wrap(mk.Shape(), "extrusion");
  });
}

std::unique_ptr<Shape> revolve(const Shape &face, P3 origin, P3 axis, double angle) {
  return guard("revolve", [&]() {
    BRepPrimAPI_MakeRevol mk(face.s, gp_Ax1(pnt(origin), gp_Dir(axis.x, axis.y, axis.z)), angle);
    mk.Build();
    if (!mk.IsDone()) {
      throw std::runtime_error("not done");
    }
    return wrap(mk.Shape(), "revolve");
  });
}

std::unique_ptr<Shape> fuse(const Shape &a, const Shape &b) {
  return guard("union", [&]() {
    BRepAlgoAPI_Fuse op(a.s, b.s);
    if (!op.IsDone() || op.HasErrors()) {
      throw std::runtime_error("boolean not done");
    }
    return wrap(checked_solid_result(unify(op.Shape()), "union"), "union");
  });
}

std::unique_ptr<Shape> cut(const Shape &a, const Shape &b) {
  return guard("cut", [&]() {
    BRepAlgoAPI_Cut op(a.s, b.s);
    if (!op.IsDone() || op.HasErrors()) {
      throw std::runtime_error("boolean not done");
    }
    return wrap(checked_solid_result(unify(op.Shape()), "cut"), "cut");
  });
}

std::unique_ptr<Shape> fillet(const Shape &s, rust::Slice<const uint32_t> edges, double radius) {
  return guard("fillet", [&]() {
    TopTools_IndexedMapOfShape map;
    TopExp::MapShapes(s.s, TopAbs_EDGE, map);
    BRepFilletAPI_MakeFillet mk(s.s);
    for (uint32_t e : edges) {
      if (e >= static_cast<uint32_t>(map.Extent())) {
        throw std::runtime_error("edge index out of range");
      }
      mk.Add(radius, TopoDS::Edge(map(static_cast<int>(e) + 1)));
    }
    mk.Build();
    if (!mk.IsDone()) {
      throw std::runtime_error("radius too large or edges not filletable");
    }
    return wrap(checked_solid_result(mk.Shape(), "fillet"), "fillet");
  });
}

std::unique_ptr<Shape> chamfer(const Shape &s, rust::Slice<const uint32_t> edges, double distance) {
  return guard("chamfer", [&]() {
    TopTools_IndexedMapOfShape map;
    TopExp::MapShapes(s.s, TopAbs_EDGE, map);
    BRepFilletAPI_MakeChamfer mk(s.s);
    for (uint32_t e : edges) {
      if (e >= static_cast<uint32_t>(map.Extent())) {
        throw std::runtime_error("edge index out of range");
      }
      mk.Add(distance, TopoDS::Edge(map(static_cast<int>(e) + 1)));
    }
    mk.Build();
    if (!mk.IsDone()) {
      throw std::runtime_error("distance too large or edges not chamferable");
    }
    return wrap(checked_solid_result(mk.Shape(), "chamfer"), "chamfer");
  });
}

MeshOut triangulate(const Shape &s, double tolerance) {
  return guard("tessellation", [&]() {
    BRepMesh_IncrementalMesh mesher(s.s, tolerance, Standard_False, 0.35, Standard_False);
    MeshOut out;
    TopTools_IndexedMapOfShape faces;
    TopExp::MapShapes(s.s, TopAbs_FACE, faces);
    for (int fi = 1; fi <= faces.Extent(); ++fi) {
      const TopoDS_Face &face = TopoDS::Face(faces(fi));
      TopLoc_Location loc;
      Handle(Poly_Triangulation) tri = BRep_Tool::Triangulation(face, loc);
      if (tri.IsNull()) {
        continue;
      }
      if (!tri->HasNormals()) {
        BRepLib_ToolTriangulatedShape::ComputeNormals(face, tri);
      }
      const gp_Trsf trsf = loc.Transformation();
      const bool reversed = face.Orientation() == TopAbs_REVERSED;
      const uint32_t base = static_cast<uint32_t>(out.positions.size() / 3);
      for (int k = 1; k <= tri->NbNodes(); ++k) {
        gp_Pnt p = tri->Node(k).Transformed(trsf);
        out.positions.push_back(static_cast<float>(p.X()));
        out.positions.push_back(static_cast<float>(p.Y()));
        out.positions.push_back(static_cast<float>(p.Z()));
        gp_Dir n = tri->Normal(k).Transformed(trsf);
        const double sign = reversed ? -1.0 : 1.0;
        out.normals.push_back(static_cast<float>(sign * n.X()));
        out.normals.push_back(static_cast<float>(sign * n.Y()));
        out.normals.push_back(static_cast<float>(sign * n.Z()));
      }
      for (int t = 1; t <= tri->NbTriangles(); ++t) {
        int a, b, c;
        tri->Triangle(t).Get(a, b, c);
        if (reversed) {
          std::swap(b, c);
        }
        out.indices.push_back(base + static_cast<uint32_t>(a - 1));
        out.indices.push_back(base + static_cast<uint32_t>(b - 1));
        out.indices.push_back(base + static_cast<uint32_t>(c - 1));
        out.tri_faces.push_back(static_cast<uint32_t>(fi - 1));
      }
    }
    // Edges in TopExp map order, so indices match `edges()` and `fillet()`
    TopTools_IndexedMapOfShape edge_map;
    TopExp::MapShapes(s.s, TopAbs_EDGE, edge_map);
    for (int ei = 1; ei <= edge_map.Extent(); ++ei) {
      const TopoDS_Edge &edge = TopoDS::Edge(edge_map(ei));
      out.edge_offsets.push_back(static_cast<uint32_t>(out.edge_points.size() / 3));
      if (BRep_Tool::Degenerated(edge)) {
        continue;
      }
      BRepAdaptor_Curve curve(edge);
      GCPnts_QuasiUniformDeflection pts(curve, tolerance);
      if (!pts.IsDone()) {
        continue;
      }
      for (int k = 1; k <= pts.NbPoints(); ++k) {
        gp_Pnt p = pts.Value(k);
        out.edge_points.push_back(static_cast<float>(p.X()));
        out.edge_points.push_back(static_cast<float>(p.Y()));
        out.edge_points.push_back(static_cast<float>(p.Z()));
      }
    }
    out.edge_offsets.push_back(static_cast<uint32_t>(out.edge_points.size() / 3));
    return out;
  });
}

double volume(const Shape &s) {
  return guard("volume", [&]() {
    GProp_GProps props;
    BRepGProp::VolumeProperties(s.s, props);
    return props.Mass();
  });
}

Bounds bounds(const Shape &s) {
  return guard("bounds", [&]() {
    Bnd_Box box;
    BRepBndLib::AddOptimal(s.s, box, Standard_False, Standard_False);
    if (box.IsVoid()) {
      return Bounds{P3{0, 0, 0}, P3{0, 0, 0}, true};
    }
    double x0, y0, z0, x1, y1, z1;
    box.Get(x0, y0, z0, x1, y1, z1);
    return Bounds{P3{x0, y0, z0}, P3{x1, y1, z1}, false};
  });
}

size_t face_count(const Shape &s) { return count(s.s, TopAbs_FACE); }

size_t solid_count(const Shape &s) { return count(s.s, TopAbs_SOLID); }

rust::Vec<EdgeData> edges(const Shape &s) {
  return guard("edge query", [&]() {
    rust::Vec<EdgeData> out;
    TopTools_IndexedMapOfShape map;
    TopExp::MapShapes(s.s, TopAbs_EDGE, map);
    for (int ei = 1; ei <= map.Extent(); ++ei) {
      const TopoDS_Edge &edge = TopoDS::Edge(map(ei));
      if (BRep_Tool::Degenerated(edge)) {
        gp_Pnt p = BRep_Tool::Pnt(TopExp::FirstVertex(edge));
        out.push_back(EdgeData{p3(p), p3(p), p3(p), true});
        continue;
      }
      BRepAdaptor_Curve curve(edge);
      double f = curve.FirstParameter(), l = curve.LastParameter();
      out.push_back(EdgeData{p3(curve.Value((f + l) / 2.0)), p3(curve.Value(f)), p3(curve.Value(l)), false});
    }
    return out;
  });
}

rust::Vec<FaceGeom> face_geometry(const Shape &s) {
  return guard("face query", [&]() {
    rust::Vec<FaceGeom> out;
    TopTools_IndexedMapOfShape faces;
    TopExp::MapShapes(s.s, TopAbs_FACE, faces);
    for (int fi = 1; fi <= faces.Extent(); ++fi) {
      const TopoDS_Face &face = TopoDS::Face(faces(fi));
      BRepAdaptor_Surface surf(face);
      double u0, u1, v0, v1;
      BRepTools::UVBounds(face, u0, u1, v0, v1);
      gp_Pnt mid = surf.Value((u0 + u1) / 2.0, (v0 + v1) / 2.0);
      FaceGeom g{2, p3(mid), P3{0, 0, 1}, 0.0, p3(mid)};
      if (surf.GetType() == GeomAbs_Plane) {
        gp_Dir n = surf.Plane().Axis().Direction();
        if (face.Orientation() == TopAbs_REVERSED) {
          n.Reverse();
        }
        g = FaceGeom{0, p3(mid), p3(n), 0.0, p3(mid)};
      } else if (surf.GetType() == GeomAbs_Cylinder) {
        gp_Cylinder c = surf.Cylinder();
        g = FaceGeom{1, p3(c.Axis().Location()), p3(c.Axis().Direction()), c.Radius(), p3(mid)};
      }
      out.push_back(g);
    }
    return out;
  });
}

std::unique_ptr<Shape> transformed(const Shape &s, rust::Slice<const double> m) {
  return guard("transform", [&]() {
    if (m.size() != 12) {
      throw std::runtime_error("expected a 3x4 matrix");
    }
    gp_Trsf t;
    t.SetValues(m[0], m[1], m[2], m[3], m[4], m[5], m[6], m[7], m[8], m[9], m[10], m[11]);
    BRepBuilderAPI_Transform op(s.s, t, Standard_True);
    return wrap(op.Shape(), "transform");
  });
}

std::unique_ptr<Shape> common(const Shape &a, const Shape &b) {
  return guard("intersection", [&]() {
    BRepAlgoAPI_Common op(a.s, b.s);
    if (!op.IsDone() || op.HasErrors()) {
      throw std::runtime_error("boolean not done");
    }
    return wrap(op.Shape(), "intersection");
  });
}

std::unique_ptr<Shape> compound_with(const Shape &c, const Shape &s) {
  return guard("compound", [&]() {
    BRep_Builder b;
    TopoDS_Compound out;
    b.MakeCompound(out);
    if (!c.s.IsNull()) {
      if (c.s.ShapeType() == TopAbs_COMPOUND) {
        for (TopoDS_Iterator it(c.s); it.More(); it.Next()) {
          b.Add(out, it.Value());
        }
      } else {
        b.Add(out, c.s);
      }
    }
    b.Add(out, s.s);
    return wrap(out, "compound");
  });
}

void write_step(const Shape &s, rust::Str path) {
  guard("STEP export", [&]() {
    // Keep the STEP translator from printing statistics to stdout
    Message::DefaultMessenger()->ChangePrinters().Clear();
    STEPControl_Writer writer;
    if (writer.Transfer(s.s, STEPControl_AsIs) != IFSelect_RetDone) {
      throw std::runtime_error("transfer failed");
    }
    std::string p(path.data(), path.size());
    if (writer.Write(p.c_str()) != IFSelect_RetDone) {
      throw std::runtime_error("cannot write " + p);
    }
    return 0;
  });
}

std::unique_ptr<Shape> clone_shape(const Shape &s) {
  auto out = std::make_unique<Shape>();
  out->s = s.s;
  return out;
}

bool is_valid(const Shape &s) {
  try {
    return !s.s.IsNull() && BRepCheck_Analyzer(s.s).IsValid();
  } catch (...) {
    return false;
  }
}

} // namespace nova

// OpenCascade adapter: build and modify solids, report their topology with
// provenance, measure them, mesh them, and write STEP. Naming, validation
// policy, and everything else stays in Rust.
#include "occt_adapter.h"

#include <algorithm>
#include <cmath>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <exception>
#include <string>
#include <utility>
#include <vector>

#include <BRepAdaptor_Curve.hxx>
#include <BRepAdaptor_Surface.hxx>
#include <BRepAlgoAPI_Cut.hxx>
#include <BRepBndLib.hxx>
#include <BRepBuilderAPI_MakeEdge.hxx>
#include <BRepBuilderAPI_MakeFace.hxx>
#include <BRepBuilderAPI_MakeShape.hxx>
#include <BRepBuilderAPI_MakeVertex.hxx>
#include <BRepBuilderAPI_MakeWire.hxx>
#include <BRepBuilderAPI_Transform.hxx>
#include <BRepCheck_Analyzer.hxx>
#include <BRepFilletAPI_MakeChamfer.hxx>
#include <BRepFilletAPI_MakeFillet.hxx>
#include <BRepGProp.hxx>
#include <BRepMesh_IncrementalMesh.hxx>
#include <BRepPrimAPI_MakeBox.hxx>
#include <BRepPrimAPI_MakeCylinder.hxx>
#include <BRepPrimAPI_MakePrism.hxx>
#include <BRepPrimAPI_MakeRevol.hxx>
#include <BRep_Builder.hxx>
#include <BRep_Tool.hxx>
#include <Bnd_Box.hxx>
#include <GCPnts_AbscissaPoint.hxx>
#include <GC_MakeArcOfCircle.hxx>
#include <GProp_GProps.hxx>
#include <Geom_TrimmedCurve.hxx>
#include <IFSelect_ReturnStatus.hxx>
#include <Interface_Static.hxx>
#include <Message.hxx>
#include <Message_Messenger.hxx>
#include <Message_PrinterOStream.hxx>
#include <NCollection_IndexedDataMap.hxx>
#include <NCollection_IndexedMap.hxx>
#include <NCollection_List.hxx>
#include <Poly_Triangulation.hxx>
#include <Quantity_Color.hxx>
#include <STEPCAFControl_Writer.hxx>
#include <STEPControl_StepModelType.hxx>
#include <Standard_Failure.hxx>
#include <Standard_Version.hxx>
#include <TCollection_ExtendedString.hxx>
#include <TDF_Label.hxx>
#include <TDataStd_Name.hxx>
#include <TDocStd_Document.hxx>
#include <TopAbs_Orientation.hxx>
#include <TopExp.hxx>
#include <TopExp_Explorer.hxx>
#include <TopLoc_Location.hxx>
#include <TopTools_ShapeMapHasher.hxx>
#include <TopoDS.hxx>
#include <TopoDS_Compound.hxx>
#include <TopoDS_Edge.hxx>
#include <TopoDS_Face.hxx>
#include <TopoDS_Shape.hxx>
#include <TopoDS_Vertex.hxx>
#include <TopoDS_Wire.hxx>
#include <XCAFApp_Application.hxx>
#include <XCAFDoc_ColorTool.hxx>
#include <XCAFDoc_DocumentTool.hxx>
#include <XCAFDoc_ShapeTool.hxx>
#include <gp_Ax1.hxx>
#include <gp_Ax2.hxx>
#include <gp_Dir.hxx>
#include <gp_Pnt.hxx>
#include <gp_Trsf.hxx>
#include <gp_Vec.hxx>

using ShapeMap = NCollection_IndexedMap<TopoDS_Shape, TopTools_ShapeMapHasher>;
using AncestorMap =
    NCollection_IndexedDataMap<TopoDS_Shape, NCollection_List<TopoDS_Shape>, TopTools_ShapeMapHasher>;

namespace {

struct Tag {
  int tag = LC_TAG_UNKNOWN;
  int index = -1;
};

struct Entry {
  TopoDS_Shape shape;
  std::vector<Tag> tags;  // per face, in TopExp map order
};

}  // namespace

struct LcSession {
  std::vector<Entry> shapes;
  int profile_plane = 0;
  // Profile loops: the face's wires and their edges in segment order.
  std::vector<TopoDS_Wire> wires;
  std::vector<std::vector<TopoDS_Edge>> loop_edges;
  std::string error;
};

namespace {

template <typename T>
T *copy_out(const std::vector<T> &values) {
  T *out = static_cast<T *>(std::malloc(std::max<size_t>(values.size(), 1) * sizeof(T)));
  if (out && !values.empty()) {
    std::memcpy(out, values.data(), values.size() * sizeof(T));
  }
  return out;
}

void silence_messages() {
  // OCCT reports progress through printers that write to std::cout. The
  // engine speaks its protocol on stdout, so every printer is removed once.
  static bool done = false;
  if (done) {
    return;
  }
  done = true;
  const Handle(Message_Messenger) &messenger = Message::DefaultMessenger();
  messenger->RemovePrinters(STANDARD_TYPE(Message_PrinterOStream));
}

void set_error(LcSession *s, const std::string &what) {
  if (s) {
    s->error = what.empty() ? "unknown OpenCascade failure" : what;
  }
}

const Entry *get(LcSession *s, int id) {
  if (!s || id < 0 || static_cast<size_t>(id) >= s->shapes.size() || s->shapes[id].shape.IsNull()) {
    return nullptr;
  }
  return &s->shapes[id];
}

int push(LcSession *s, const TopoDS_Shape &shape, std::vector<Tag> tags) {
  s->shapes.push_back(Entry{shape, std::move(tags)});
  return static_cast<int>(s->shapes.size() - 1);
}

ShapeMap faces_of(const TopoDS_Shape &shape) {
  ShapeMap m;
  TopExp::MapShapes(shape, TopAbs_FACE, m);
  return m;
}

ShapeMap edges_of(const TopoDS_Shape &shape) {
  ShapeMap m;
  TopExp::MapShapes(shape, TopAbs_EDGE, m);
  return m;
}

gp_Pnt plane_point(int plane, double x, double y) {
  return plane == 1 ? gp_Pnt(x, 0.0, y) : gp_Pnt(x, y, 0.0);
}

void mark(std::vector<Tag> &tags, const ShapeMap &out, const TopoDS_Shape &face, int tag, int index) {
  const int k = out.FindIndex(face);
  if (k > 0 && tags[k - 1].tag == LC_TAG_UNKNOWN) {
    tags[k - 1] = Tag{tag, index};
  }
}

// Provenance of every face of `maker`'s result relative to its inputs.
std::vector<Tag> history(BRepBuilderAPI_MakeShape &maker, const TopoDS_Shape &result,
                         const TopoDS_Shape &input, int edge_tag, const TopoDS_Shape *tool,
                         int tool_tag, bool tool_by_index) {
  const ShapeMap out = faces_of(result);
  std::vector<Tag> tags(out.Extent());
  const ShapeMap in_faces = faces_of(input);
  for (int i = 1; i <= in_faces.Extent(); ++i) {
    const TopoDS_Shape &f = in_faces(i);
    if (maker.IsDeleted(f)) {
      continue;
    }
    const NCollection_List<TopoDS_Shape> &mod = maker.Modified(f);
    if (mod.IsEmpty()) {
      mark(tags, out, f, LC_TAG_KEPT, i - 1);
    } else {
      for (const TopoDS_Shape &m : mod) {
        mark(tags, out, m, LC_TAG_KEPT, i - 1);
      }
    }
  }
  if (edge_tag != LC_TAG_UNKNOWN) {
    const ShapeMap in_edges = edges_of(input);
    for (int i = 1; i <= in_edges.Extent(); ++i) {
      for (const TopoDS_Shape &g : maker.Generated(in_edges(i))) {
        if (g.ShapeType() == TopAbs_FACE) {
          mark(tags, out, g, edge_tag, i - 1);
        }
      }
    }
  }
  if (tool) {
    const ShapeMap tool_faces = faces_of(*tool);
    for (int i = 1; i <= tool_faces.Extent(); ++i) {
      const TopoDS_Shape &f = tool_faces(i);
      const int index = tool_by_index ? i - 1 : 0;
      const NCollection_List<TopoDS_Shape> &mod = maker.Modified(f);
      if (mod.IsEmpty()) {
        mark(tags, out, f, tool_tag, index);
      } else {
        for (const TopoDS_Shape &m : mod) {
          mark(tags, out, m, tool_tag, index);
        }
      }
    }
  }
  return tags;
}

int surface_kind(const BRepAdaptor_Surface &s) {
  switch (s.GetType()) {
    case GeomAbs_Plane:
      return LC_SURF_PLANE;
    case GeomAbs_Cylinder:
      return LC_SURF_CYLINDER;
    case GeomAbs_Cone:
      return LC_SURF_CONE;
    case GeomAbs_Sphere:
      return LC_SURF_SPHERE;
    case GeomAbs_Torus:
      return LC_SURF_TORUS;
    default:
      return LC_SURF_OTHER;
  }
}

bool surface_normal(const TopoDS_Face &face, double u, double v, gp_Vec &n) {
  BRepAdaptor_Surface s(face);
  gp_Pnt p;
  gp_Vec du, dv;
  s.D1(u, v, p, du, dv);
  n = du.Crossed(dv);
  if (n.Magnitude() < 1e-12) {
    return false;
  }
  n.Normalize();
  if (face.Orientation() == TopAbs_REVERSED) {
    n.Reverse();
  }
  return true;
}

template <typename F>
int guarded(LcSession *s, F &&body) {
  try {
    return body();
  } catch (const Standard_Failure &failure) {
    set_error(s, failure.GetMessageString() ? failure.GetMessageString() : "OpenCascade failure");
  } catch (const std::exception &e) {
    set_error(s, e.what());
  } catch (...) {
    set_error(s, "unknown OpenCascade failure");
  }
  return -1;
}

int check_result(LcSession *s, const TopoDS_Shape &shape, const char *what) {
  if (shape.IsNull()) {
    set_error(s, std::string(what) + " produced no shape");
    return 0;
  }
  BRepCheck_Analyzer analyzer(shape);
  if (!analyzer.IsValid()) {
    set_error(s, std::string(what) + " produced an invalid solid");
    return 0;
  }
  return 1;
}

}  // namespace

extern "C" {

int lc_kernel_version(char *buffer, size_t cap) {
  if (!buffer || cap == 0) {
    return 1;
  }
  std::snprintf(buffer, cap, "OpenCascade %s", OCC_VERSION_COMPLETE);
  return 0;
}

LcSession *lc_session_new(void) {
  silence_messages();
  return new LcSession();
}

void lc_session_free(LcSession *session) { delete session; }

const char *lc_last_error(const LcSession *session) {
  return session ? session->error.c_str() : "no session";
}

void lc_profile_begin(LcSession *s, int plane) {
  if (!s) {
    return;
  }
  s->profile_plane = plane;
  s->wires.clear();
  s->loop_edges.clear();
}

int lc_profile_loop(LcSession *s, const double *xyb, size_t n) {
  if (!s || !xyb || n < 2) {
    set_error(s, "profile loop needs at least two vertices");
    return 1;
  }
  return guarded(s, [&]() -> int {
    std::vector<TopoDS_Vertex> vertices;
    for (size_t i = 0; i < n; ++i) {
      vertices.push_back(
          BRepBuilderAPI_MakeVertex(plane_point(s->profile_plane, xyb[3 * i], xyb[3 * i + 1])));
    }
    std::vector<TopoDS_Edge> edges;
    BRepBuilderAPI_MakeWire wire;
    for (size_t i = 0; i < n; ++i) {
      const size_t j = (i + 1) % n;
      const double ax = xyb[3 * i], ay = xyb[3 * i + 1], bulge = xyb[3 * i + 2];
      const double bx = xyb[3 * j], by = xyb[3 * j + 1];
      TopoDS_Edge edge;
      if (bulge == 0.0) {
        edge = BRepBuilderAPI_MakeEdge(vertices[i], vertices[j]);
      } else {
        // Arc midpoint: sagitta bulge * chord / 2, to the left of a->b for
        // a positive (counter-clockwise) bulge.
        const double cx = bx - ax, cy = by - ay;
        const double len = std::sqrt(cx * cx + cy * cy);
        const double sag = bulge * len * 0.5;
        const double mx = (ax + bx) * 0.5 - cy / len * sag;
        const double my = (ay + by) * 0.5 + cx / len * sag;
        GC_MakeArcOfCircle arc(plane_point(s->profile_plane, ax, ay),
                               plane_point(s->profile_plane, mx, my),
                               plane_point(s->profile_plane, bx, by));
        if (!arc.IsDone()) {
          set_error(s, "could not build an arc in the profile");
          return 1;
        }
        edge = BRepBuilderAPI_MakeEdge(arc.Value(), vertices[i], vertices[j]);
      }
      edges.push_back(edge);
      wire.Add(edge);
    }
    if (!wire.IsDone()) {
      set_error(s, "profile loop does not close");
      return 1;
    }
    s->wires.push_back(wire.Wire());
    s->loop_edges.push_back(edges);
    return 0;
  });
}

int lc_make_box(LcSession *s, const double size[3]) {
  if (!s || !size) {
    return -1;
  }
  return guarded(s, [&]() -> int {
    BRepPrimAPI_MakeBox maker(gp_Pnt(0, 0, 0), size[0], size[1], size[2]);
    const TopoDS_Shape solid = maker.Solid();
    const ShapeMap out = faces_of(solid);
    std::vector<Tag> tags(out.Extent());
    for (int i = 1; i <= out.Extent(); ++i) {
      const TopoDS_Face f = TopoDS::Face(out(i));
      BRepAdaptor_Surface surf(f);
      gp_Vec n;
      surface_normal(f, (surf.FirstUParameter() + surf.LastUParameter()) * 0.5,
                     (surf.FirstVParameter() + surf.LastVParameter()) * 0.5, n);
      int axis = 0;
      for (int k = 1; k < 3; ++k) {
        if (std::abs(n.Coord(k + 1)) > std::abs(n.Coord(axis + 1))) {
          axis = k;
        }
      }
      tags[i - 1] = Tag{LC_TAG_BOX, axis * 2 + (n.Coord(axis + 1) > 0 ? 1 : 0)};
    }
    return push(s, solid, tags);
  });
}

static int sweep(LcSession *s, bool revolve, double amount) {
  if (!s || s->wires.empty()) {
    set_error(s, "no profile");
    return -1;
  }
  return guarded(s, [&]() -> int {
    BRepBuilderAPI_MakeFace face_maker(s->wires[0], Standard_True);
    for (size_t h = 1; h < s->wires.size(); ++h) {
      face_maker.Add(s->wires[h]);
    }
    if (!face_maker.IsDone()) {
      set_error(s, "profile is not a planar face");
      return -1;
    }
    const TopoDS_Face face = face_maker.Face();
    TopoDS_Shape result;
    std::vector<Tag> tags;
    auto tag_sides = [&](BRepPrimAPI_MakeSweep &maker, int side_tag) {
      const ShapeMap out = faces_of(result);
      tags.assign(out.Extent(), Tag{});
      for (size_t l = 0; l < s->loop_edges.size(); ++l) {
        for (size_t i = 0; i < s->loop_edges[l].size(); ++i) {
          for (const TopoDS_Shape &g : maker.Generated(s->loop_edges[l][i])) {
            if (g.ShapeType() == TopAbs_FACE) {
              mark(tags, out, g, side_tag, static_cast<int>(l * 10000 + i));
            }
          }
        }
      }
      return out;
    };
    if (!revolve) {
      gp_Vec dir = s->profile_plane == 1 ? gp_Vec(0, -amount, 0) : gp_Vec(0, 0, amount);
      BRepPrimAPI_MakePrism maker(face, dir, Standard_True);
      maker.Build();
      if (!maker.IsDone()) {
        set_error(s, "extrude failed");
        return -1;
      }
      result = maker.Shape();
      const ShapeMap out = tag_sides(maker, LC_TAG_SIDE);
      mark(tags, out, maker.FirstShape(), LC_TAG_BOTTOM, 0);
      mark(tags, out, maker.LastShape(), LC_TAG_TOP, 0);
    } else {
      const double angle = amount * M_PI / 180.0;
      BRepPrimAPI_MakeRevol maker(face, gp_Ax1(gp_Pnt(0, 0, 0), gp_Dir(0, 0, 1)), angle,
                                  Standard_True);
      maker.Build();
      if (!maker.IsDone()) {
        set_error(s, "revolve failed");
        return -1;
      }
      result = maker.Shape();
      const ShapeMap out = tag_sides(maker, LC_TAG_REV_SIDE);
      if (amount < 360.0 - 1e-9) {
        mark(tags, out, maker.FirstShape(), LC_TAG_REV_START, 0);
        mark(tags, out, maker.LastShape(), LC_TAG_REV_END, 0);
      }
    }
    if (!check_result(s, result, revolve ? "revolve" : "extrude")) {
      return -1;
    }
    return push(s, result, tags);
  });
}

int lc_make_prism(LcSession *s, double depth) { return sweep(s, false, depth); }

int lc_make_revol(LcSession *s, double angle_deg) { return sweep(s, true, angle_deg); }

static int edge_op(LcSession *s, int shape, const int *edges, size_t n, double size, bool fillet) {
  const Entry *in = get(s, shape);
  if (!in || !edges || n == 0) {
    set_error(s, "edge operation needs a shape and at least one edge");
    return -1;
  }
  const TopoDS_Shape input = in->shape;
  return guarded(s, [&]() -> int {
    const ShapeMap all = edges_of(input);
    auto edge_at = [&](int idx) -> const TopoDS_Edge * {
      if (idx < 0 || idx >= all.Extent()) {
        return nullptr;
      }
      return &TopoDS::Edge(all(idx + 1));
    };
    TopoDS_Shape result;
    std::vector<Tag> tags;
    if (fillet) {
      BRepFilletAPI_MakeFillet maker(input);
      for (size_t k = 0; k < n; ++k) {
        const TopoDS_Edge *e = edge_at(edges[k]);
        if (!e) {
          set_error(s, "fillet edge index out of range");
          return -1;
        }
        maker.Add(size, *e);
      }
      maker.Build();
      if (!maker.IsDone()) {
        set_error(s, "fillet failed: the radius is too large for the faces next to that edge");
        return -1;
      }
      result = maker.Shape();
      tags = history(maker, result, input, LC_TAG_FROM_EDGE, nullptr, 0, false);
    } else {
      BRepFilletAPI_MakeChamfer maker(input);
      for (size_t k = 0; k < n; ++k) {
        const TopoDS_Edge *e = edge_at(edges[k]);
        if (!e) {
          set_error(s, "chamfer edge index out of range");
          return -1;
        }
        maker.Add(size, *e);
      }
      maker.Build();
      if (!maker.IsDone()) {
        set_error(s, "chamfer failed: the distance is too large for the faces next to that edge");
        return -1;
      }
      result = maker.Shape();
      tags = history(maker, result, input, LC_TAG_FROM_EDGE, nullptr, 0, false);
    }
    if (!check_result(s, result, fillet ? "fillet" : "chamfer")) {
      return -1;
    }
    return push(s, result, tags);
  });
}

int lc_fillet(LcSession *s, int shape, const int *edges, size_t n, double radius) {
  return edge_op(s, shape, edges, n, radius, true);
}

int lc_chamfer(LcSession *s, int shape, const int *edges, size_t n, double distance) {
  return edge_op(s, shape, edges, n, distance, false);
}

static int cut_with(LcSession *s, const TopoDS_Shape &input, const TopoDS_Shape &tool, int tool_tag,
                    bool by_index, const char *what) {
  BRepAlgoAPI_Cut maker(input, tool);
  maker.Build();
  if (!maker.IsDone() || maker.HasErrors()) {
    set_error(s, std::string(what) + " failed");
    return -1;
  }
  const TopoDS_Shape result = maker.Shape();
  if (!check_result(s, result, what)) {
    return -1;
  }
  std::vector<Tag> tags = history(maker, result, input, LC_TAG_UNKNOWN, &tool, tool_tag, by_index);
  return push(s, result, tags);
}

int lc_hole(LcSession *s, int shape, const double origin[3], const double dir[3], double diameter,
            double depth) {
  const Entry *in = get(s, shape);
  if (!in || !origin || !dir || !(diameter > 0)) {
    set_error(s, "hole needs a shape, a position, a direction, and a diameter");
    return -1;
  }
  const TopoDS_Shape input = in->shape;
  return guarded(s, [&]() -> int {
    const gp_Dir d(dir[0], dir[1], dir[2]);
    const gp_Pnt o(origin[0], origin[1], origin[2]);
    double start_back = 1e-3, length = depth + 1e-3;
    if (depth <= 0) {
      Bnd_Box box;
      BRepBndLib::Add(input, box);
      const double diag = std::sqrt(box.SquareExtent());
      start_back = diag;
      length = 2.0 * diag;
    }
    const gp_Pnt base = o.Translated(gp_Vec(d) * -start_back);
    const TopoDS_Shape tool = BRepPrimAPI_MakeCylinder(gp_Ax2(base, d), diameter * 0.5, length).Shape();
    // Tool faces: the cylinder wall is the hole wall; its end caps can
    // only survive as a blind hole's bottom.
    const int id = cut_with(s, input, tool, LC_TAG_HOLE, true, "hole");
    if (id >= 0) {
      const ShapeMap out = faces_of(s->shapes[id].shape);
      for (int i = 1; i <= out.Extent(); ++i) {
        Tag &t = s->shapes[id].tags[i - 1];
        if (t.tag == LC_TAG_HOLE) {
          BRepAdaptor_Surface surf(TopoDS::Face(out(i)));
          t.index = surf.GetType() == GeomAbs_Plane ? 1 : 0;
        }
      }
    }
    return id;
  });
}

int lc_cut(LcSession *s, int shape, int tool) {
  const Entry *in = get(s, shape);
  const Entry *t = get(s, tool);
  if (!in || !t) {
    set_error(s, "cut needs two shapes");
    return -1;
  }
  const TopoDS_Shape input = in->shape, tool_shape = t->shape;
  return guarded(s, [&]() -> int { return cut_with(s, input, tool_shape, LC_TAG_TOOL, true, "cut"); });
}

int lc_transform(LcSession *s, int shape, const double m[12], double scale) {
  const Entry *in = get(s, shape);
  if (!in || !m) {
    return -1;
  }
  const TopoDS_Shape input = in->shape;
  const std::vector<Tag> tags = in->tags;
  return guarded(s, [&]() -> int {
    gp_Trsf rigid;
    rigid.SetValues(m[0], m[1], m[2], m[3], m[4], m[5], m[6], m[7], m[8], m[9], m[10], m[11]);
    gp_Trsf scaling;
    scaling.SetScale(gp_Pnt(0, 0, 0), scale);
    BRepBuilderAPI_Transform t(input, scaling * rigid, Standard_True);
    // Face order is preserved by a copy, so the tags carry over.
    return push(s, t.Shape(), tags);
  });
}

void lc_release(LcSession *s, int shape) {
  if (s && shape >= 0 && static_cast<size_t>(shape) < s->shapes.size()) {
    s->shapes[shape].shape.Nullify();
    s->shapes[shape].tags.clear();
  }
}

int lc_check(LcSession *s, int shape) {
  const Entry *e = get(s, shape);
  if (!e) {
    return 0;
  }
  try {
    BRepCheck_Analyzer analyzer(e->shape);
    return analyzer.IsValid() ? 1 : 0;
  } catch (...) {
    return 0;
  }
}

int lc_face_count(LcSession *s, int shape) {
  const Entry *e = get(s, shape);
  return e ? faces_of(e->shape).Extent() : -1;
}

int lc_edge_count(LcSession *s, int shape) {
  const Entry *e = get(s, shape);
  return e ? edges_of(e->shape).Extent() : -1;
}

int lc_topology(LcSession *s, int shape, LcFace *faces, LcEdge *edges) {
  const Entry *e = get(s, shape);
  if (!e || !faces || !edges) {
    return 1;
  }
  return guarded(s, [&]() -> int {
    const ShapeMap fmap = faces_of(e->shape);
    for (int i = 1; i <= fmap.Extent(); ++i) {
      const TopoDS_Face f = TopoDS::Face(fmap(i));
      BRepAdaptor_Surface surf(f);
      GProp_GProps props;
      BRepGProp::SurfaceProperties(f, props);
      const gp_Pnt c = props.CentreOfMass();
      gp_Vec n(0, 0, 1);
      surface_normal(f, (surf.FirstUParameter() + surf.LastUParameter()) * 0.5,
                     (surf.FirstVParameter() + surf.LastVParameter()) * 0.5, n);
      LcFace &out = faces[i - 1];
      out.surface = surface_kind(surf);
      out.tag = e->tags.size() >= static_cast<size_t>(i) ? e->tags[i - 1].tag : LC_TAG_UNKNOWN;
      out.index = e->tags.size() >= static_cast<size_t>(i) ? e->tags[i - 1].index : -1;
      out.area = props.Mass();
      out.centroid[0] = c.X();
      out.centroid[1] = c.Y();
      out.centroid[2] = c.Z();
      out.normal[0] = n.X();
      out.normal[1] = n.Y();
      out.normal[2] = n.Z();
    }
    AncestorMap anc;
    TopExp::MapShapesAndAncestors(e->shape, TopAbs_EDGE, TopAbs_FACE, anc);
    const ShapeMap emap = edges_of(e->shape);
    for (int i = 1; i <= emap.Extent(); ++i) {
      const TopoDS_Edge edge = TopoDS::Edge(emap(i));
      LcEdge &out = edges[i - 1];
      out.face_a = out.face_b = -1;
      if (BRep_Tool::Degenerated(edge)) {
        out.curve = LC_CURVE_OTHER;
        out.length = 0;
      } else {
        BRepAdaptor_Curve curve(edge);
        out.length = GCPnts_AbscissaPoint::Length(curve);
        switch (curve.GetType()) {
          case GeomAbs_Line:
            out.curve = LC_CURVE_LINE;
            break;
          case GeomAbs_Circle:
            out.curve = std::abs(curve.LastParameter() - curve.FirstParameter() - 2 * M_PI) < 1e-6
                            ? LC_CURVE_CIRCLE
                            : LC_CURVE_ARC;
            break;
          default:
            out.curve = LC_CURVE_OTHER;
        }
      }
      const int k = anc.FindIndex(edge);
      if (k > 0) {
        int slot = 0;
        for (const TopoDS_Shape &f : anc(k)) {
          const int fi = fmap.FindIndex(f) - 1;
          if (slot == 0) {
            out.face_a = fi;
            out.face_b = fi;
          } else if (fi != out.face_a) {
            out.face_b = fi;
          }
          ++slot;
        }
      }
    }
    return 0;
  }) == 0
             ? 0
             : 1;
}

int lc_edge_polyline(LcSession *s, int shape, int edge, double *out, size_t cap) {
  const Entry *e = get(s, shape);
  if (!e || !out || cap < 2) {
    return 0;
  }
  try {
    const ShapeMap emap = edges_of(e->shape);
    if (edge < 0 || edge >= emap.Extent()) {
      return 0;
    }
    const TopoDS_Edge ed = TopoDS::Edge(emap(edge + 1));
    if (BRep_Tool::Degenerated(ed)) {
      return 0;
    }
    BRepAdaptor_Curve curve(ed);
    const size_t count = curve.GetType() == GeomAbs_Line ? 2 : std::min<size_t>(cap, 33);
    const double a = curve.FirstParameter(), b = curve.LastParameter();
    for (size_t k = 0; k < count; ++k) {
      const gp_Pnt p = curve.Value(a + (b - a) * static_cast<double>(k) / static_cast<double>(count - 1));
      out[3 * k] = p.X();
      out[3 * k + 1] = p.Y();
      out[3 * k + 2] = p.Z();
    }
    return static_cast<int>(count);
  } catch (...) {
    return 0;
  }
}

int lc_mass(LcSession *s, int shape, double out[13]) {
  const Entry *e = get(s, shape);
  if (!e || !out) {
    return 1;
  }
  return guarded(s, [&]() -> int {
    GProp_GProps props;
    BRepGProp::VolumeProperties(e->shape, props);
    const gp_Pnt c = props.CentreOfMass();
    const gp_Mat m = props.MatrixOfInertia();
    out[0] = props.Mass();
    out[1] = c.X();
    out[2] = c.Y();
    out[3] = c.Z();
    for (int r = 0; r < 3; ++r) {
      for (int k = 0; k < 3; ++k) {
        out[4 + 3 * r + k] = m.Value(r + 1, k + 1);
      }
    }
    return 0;
  }) == 0
             ? 0
             : 1;
}

int lc_tessellate(LcSession *s, int shape, double deflection, LcMesh *out) {
  const Entry *e = get(s, shape);
  if (!e || !out) {
    return 1;
  }
  return guarded(s, [&]() -> int {
    const TopoDS_Shape &sh = e->shape;
    BRepMesh_IncrementalMesh mesher(sh, deflection, Standard_False, 0.3, Standard_True);
    std::vector<float> positions, normals;
    std::vector<uint32_t> indices, starts;
    const ShapeMap fmap = faces_of(sh);
    for (int fi = 1; fi <= fmap.Extent(); ++fi) {
      starts.push_back(static_cast<uint32_t>(indices.size() / 3));
      const TopoDS_Face face = TopoDS::Face(fmap(fi));
      TopLoc_Location loc;
      Handle(Poly_Triangulation) tri = BRep_Tool::Triangulation(face, loc);
      if (tri.IsNull()) {
        continue;
      }
      const bool reversed = face.Orientation() == TopAbs_REVERSED;
      BRepAdaptor_Surface surf(face);
      const bool planar = surf.GetType() == GeomAbs_Plane;
      const bool uv = tri->HasUVNodes();
      const uint32_t base = static_cast<uint32_t>(positions.size() / 3);
      gp_Vec flat(0, 0, 1);
      if (planar) {
        surface_normal(face, (surf.FirstUParameter() + surf.LastUParameter()) * 0.5,
                       (surf.FirstVParameter() + surf.LastVParameter()) * 0.5, flat);
      }
      for (int k = 1; k <= tri->NbNodes(); ++k) {
        const gp_Pnt p = tri->Node(k).Transformed(loc.Transformation());
        positions.push_back(static_cast<float>(p.X()));
        positions.push_back(static_cast<float>(p.Y()));
        positions.push_back(static_cast<float>(p.Z()));
        gp_Vec n = flat;
        if (!planar && uv) {
          const gp_Pnt2d q = tri->UVNode(k);
          if (!surface_normal(face, q.X(), q.Y(), n)) {
            n = flat;
          }
        }
        normals.push_back(static_cast<float>(n.X()));
        normals.push_back(static_cast<float>(n.Y()));
        normals.push_back(static_cast<float>(n.Z()));
      }
      for (int t = 1; t <= tri->NbTriangles(); ++t) {
        int n1 = 0, n2 = 0, n3 = 0;
        tri->Triangle(t).Get(n1, n2, n3);
        if (reversed) {
          std::swap(n2, n3);
        }
        indices.push_back(base + static_cast<uint32_t>(n1 - 1));
        indices.push_back(base + static_cast<uint32_t>(n2 - 1));
        indices.push_back(base + static_cast<uint32_t>(n3 - 1));
      }
    }
    starts.push_back(static_cast<uint32_t>(indices.size() / 3));
    out->vertex_count = positions.size() / 3;
    out->index_count = indices.size();
    out->face_count = fmap.Extent();
    out->positions = copy_out(positions);
    out->normals = copy_out(normals);
    out->indices = copy_out(indices);
    out->face_tri_start = copy_out(starts);
    return 0;
  }) == 0
             ? 0
             : 2;
}

void lc_mesh_free(LcMesh *mesh) {
  if (!mesh) {
    return;
  }
  std::free(mesh->positions);
  std::free(mesh->normals);
  std::free(mesh->indices);
  std::free(mesh->face_tri_start);
  mesh->positions = nullptr;
  mesh->normals = nullptr;
  mesh->indices = nullptr;
  mesh->face_tri_start = nullptr;
}

int lc_write_step(LcSession *s, const int *ids, const char *const *names, const double *rgb, size_t n,
                  const char *path, const char *product) {
  if (!s || !path || (n > 0 && (!ids || !names || !rgb))) {
    return 1;
  }
  const int rc = guarded(s, [&]() -> int {
    // An XCAF document carries each solid's name and colour into AP214,
    // matching what the Rust writer emits.
    Handle(XCAFApp_Application) app = XCAFApp_Application::GetApplication();
    Handle(TDocStd_Document) doc;
    app->NewDocument("MDTV-XCAF", doc);
    Handle(XCAFDoc_ShapeTool) shapes = XCAFDoc_DocumentTool::ShapeTool(doc->Main());
    Handle(XCAFDoc_ColorTool) colors = XCAFDoc_DocumentTool::ColorTool(doc->Main());
    const TDF_Label root = shapes->NewShape();
    TDataStd_Name::Set(root, TCollection_ExtendedString(product ? product : "liquidCAD model", true));
    for (size_t i = 0; i < n; ++i) {
      const Entry *e = get(s, ids[i]);
      if (!e) {
        set_error(s, "STEP export references a released shape");
        app->Close(doc);
        return -1;
      }
      const TDF_Label part = shapes->AddShape(e->shape, Standard_False);
      const TCollection_ExtendedString name(names[i] ? names[i] : "part", true);
      TDataStd_Name::Set(part, name);
      colors->SetColor(part, Quantity_Color(rgb[3 * i], rgb[3 * i + 1], rgb[3 * i + 2], Quantity_TOC_sRGB),
                       XCAFDoc_ColorSurf);
      const TDF_Label instance = shapes->AddComponent(root, part, TopLoc_Location());
      TDataStd_Name::Set(instance, name);
    }
    shapes->UpdateAssemblies();
    Interface_Static::SetCVal("write.step.schema", "AP214IS");
    Interface_Static::SetCVal("write.step.unit", "MM");
    STEPCAFControl_Writer writer;
    writer.SetNameMode(Standard_True);
    writer.SetColorMode(Standard_True);
    if (!writer.Transfer(doc, STEPControl_AsIs)) {
      set_error(s, "STEP transfer failed");
      app->Close(doc);
      return -1;
    }
    const IFSelect_ReturnStatus status = writer.Write(path);
    app->Close(doc);
    if (status != IFSelect_RetDone) {
      set_error(s, "STEP write failed");
      return -1;
    }
    return 0;
  });
  return rc == 0 ? 0 : 2;
}

}  // extern "C"

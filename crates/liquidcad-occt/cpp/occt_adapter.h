// Minimal C ABI over OpenCascade. Compiled only when build.rs finds OCCT.
//
// Shapes live in a session and are addressed by integer ids. Geometry is in
// the caller's units (liquidCAD passes inches; STEP export scales to mm).
// Every operation that makes a shape also records, for each face of the
// result, where that face came from (see LcFace.tag), so the caller can name
// faces after how they were built and keep those names across operations.
#pragma once

#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef struct LcSession LcSession;

/* Face provenance tags. `index` meaning depends on the tag. */
enum {
  LC_TAG_UNKNOWN = 0,
  LC_TAG_BOTTOM = 1,      /* prism start face */
  LC_TAG_TOP = 2,         /* prism end face */
  LC_TAG_SIDE = 3,        /* prism side; index = loop * 10000 + segment */
  LC_TAG_BOX = 4,         /* box face; index 0..5 = x-, x+, y-, y+, z-, z+ */
  LC_TAG_KEPT = 5,        /* from input face `index` (unchanged or modified) */
  LC_TAG_FROM_EDGE = 6,   /* generated from input edge `index` (fillet/chamfer) */
  LC_TAG_HOLE = 7,        /* made by a hole tool; index 0 = wall, 1 = bottom */
  LC_TAG_TOOL = 8,        /* from cut-tool face `index` */
  LC_TAG_REV_SIDE = 9,    /* revolve side; index = loop * 10000 + segment */
  LC_TAG_REV_START = 10,
  LC_TAG_REV_END = 11
};

enum { LC_SURF_PLANE = 0, LC_SURF_CYLINDER, LC_SURF_CONE, LC_SURF_SPHERE, LC_SURF_TORUS, LC_SURF_OTHER };
enum { LC_CURVE_LINE = 0, LC_CURVE_CIRCLE, LC_CURVE_ARC, LC_CURVE_OTHER };

typedef struct LcFace {
  int surface;
  int tag;
  int index;
  double area;
  double centroid[3];
  double normal[3];
} LcFace;

typedef struct LcEdge {
  int curve;
  int face_a; /* adjacent faces (indices into the face list); equal on a seam */
  int face_b;
  double length;
} LcEdge;

typedef struct LcMesh {
  float *positions;
  float *normals;
  uint32_t *indices;
  uint32_t *face_tri_start; /* first triangle of each face, face_count + 1 entries */
  size_t vertex_count;
  size_t index_count;
  size_t face_count;
} LcMesh;

int lc_kernel_version(char *buffer, size_t cap);

LcSession *lc_session_new(void);
void lc_session_free(LcSession *session);
const char *lc_last_error(const LcSession *session);

/* Profiles: begin, then one call per loop (first outer, then holes) with
   n vertices of (x, y, bulge). plane 0 = XY, 1 = XZ (x, y) -> (x, 0, y). */
void lc_profile_begin(LcSession *session, int plane);
int lc_profile_loop(LcSession *session, const double *xyb, size_t n);

/* Constructors return a shape id, or -1 (see lc_last_error). */
int lc_make_box(LcSession *session, const double size[3]);
int lc_make_prism(LcSession *session, double depth);
int lc_make_revol(LcSession *session, double angle_deg);
int lc_fillet(LcSession *session, int shape, const int *edges, size_t n, double radius);
int lc_chamfer(LcSession *session, int shape, const int *edges, size_t n, double distance);
/* Round hole from `origin` along `dir` (unit). depth 0 = through all. */
int lc_hole(LcSession *session, int shape, const double origin[3], const double dir[3],
            double diameter, double depth);
int lc_cut(LcSession *session, int shape, int tool);
/* Row-major [R|t] 3x4 matrix, then uniform `scale` about the origin. */
int lc_transform(LcSession *session, int shape, const double m[12], double scale);
void lc_release(LcSession *session, int shape);

/* 1 when BRepCheck_Analyzer reports the shape valid, 0 otherwise. */
int lc_check(LcSession *session, int shape);

int lc_face_count(LcSession *session, int shape);
int lc_edge_count(LcSession *session, int shape);
/* Fills faces[face_count] and edges[edge_count] in TopExp map order. */
int lc_topology(LcSession *session, int shape, LcFace *faces, LcEdge *edges);
/* Up to `cap` xyz points along edge `edge`; returns the count written. */
int lc_edge_polyline(LcSession *session, int shape, int edge, double *out, size_t cap);

/* Volume, centroid[3], and inertia about the centroid (row-major 3x3) of a
   unit-density solid: out[13]. */
int lc_mass(LcSession *session, int shape, double out[13]);

int lc_tessellate(LcSession *session, int shape, double deflection, LcMesh *out);
void lc_mesh_free(LcMesh *mesh);

/* Writes shapes[n] as named, coloured parts of one AP214 assembly. */
int lc_write_step(LcSession *session, const int *shapes, const char *const *names,
                  const double *rgb, size_t n, const char *path, const char *product);

#ifdef __cplusplus
}
#endif

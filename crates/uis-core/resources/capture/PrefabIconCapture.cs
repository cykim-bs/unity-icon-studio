// PrefabIconCapture.cs - render prefabs (and model files) to PNG from outside the Unity Editor.
//
// One file, two entry points that share the same body:
//   * Open editor (official Unity CLI + com.unity.pipeline):
//       unity command "script run" --project-path <project> --file PrefabIconCapture.cs
//             --entry UnityIconStudio.PrefabIconCapture.RunJobFile --args '["<job.json>"]'
//     Compiled in memory; nothing is written into the project.
//   * Closed project (batch mode, a graphics device is required: do NOT pass -nographics):
//       copy this file and UnityIconStudio.Capture.Editor.asmdef to
//       Assets/__UnityIconStudioCapture/Editor/, then
//       Unity.exe -batchmode -quit -projectPath <project> -logFile <log>
//             -executeMethod UnityIconStudio.PrefabIconCapture.RunFromCommandLine
//             -iconStudioJob <job.json>
//     The asmdef keeps the file in its own editor-only assembly, so the project's other
//     assemblies (and their compile errors or defines) do not change it.
//     Exit codes: 0 all items ok, 2 some items failed, 3 job could not be read, 4 unexpected exception,
//     5 no graphics device (-nographics).
//
// "prefab" in an item is an asset path: a .prefab or a model file (.fbx, .obj, .blend, .dae), which
// Unity imports as a model prefab.
//
// The job is a JSON file (schema "unity-icon-studio.capture/1"); see the bottom of this file for an example.
// The result JSON (path in job.resultPath, default <outputDir>/result.json) lists bounds, timings,
// alpha statistics and errors per item.
//
// Design notes
//   * Prefabs are instantiated into an editor preview scene (EditorSceneManager.NewPreviewScene),
//     so the user's open scenes, selection and assets are not touched.
//   * Rendering goes through the active render pipeline (Built-in, URP or HDRP) with Camera.Render or,
//     when supported, RenderPipeline.SubmitRenderRequest(StandardRequest). Pipeline specific settings
//     (URP post processing, anti-aliasing) are set through reflection so the file compiles without URP.
//   * Background modes: "transparent" (clear to alpha 0), "solid" (opaque color), "matte" (render on black
//     and on white, recover alpha from the difference; works even when the pipeline writes alpha = 1).
//   * Optional render pipeline overrides (renderer features off, post-process alpha output on) are applied
//     in memory only and restored in a finally block. Never saved.

#if UNITY_EDITOR
using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.Globalization;
using System.IO;
using System.Linq;
using System.Reflection;
using System.Text;
using UnityEditor;
using UnityEditor.SceneManagement;
using UnityEngine;
using UnityEngine.Rendering;
using UnityEngine.SceneManagement;
using Debug = UnityEngine.Debug;
using Object = UnityEngine.Object;

namespace UnityIconStudio
{
    public static class PrefabIconCapture
    {
        public const string Schema = "unity-icon-studio.capture/1";
        const string LogPrefix = "[IconStudio] ";

        // ------------------------------------------------------------------ entry points

        /// <summary>Open-editor entry. Returns the result JSON (also written to resultPath).</summary>
        public static string RunJobFile(string jobPath)
        {
            Dictionary<string, object> job;
            try
            {
                job = Json.Parse(File.ReadAllText(jobPath, Encoding.UTF8)) as Dictionary<string, object>;
                if (job == null) throw new Exception("job root is not an object");
            }
            catch (Exception e)
            {
                return "{\"schema\":\"" + Schema + "\",\"ok\":false,\"error\":" + Json.Quote("job_read: " + e.Message) + "}";
            }
            var baseDir = Path.GetDirectoryName(Path.GetFullPath(jobPath));
            return RunJob(job, baseDir, out _);
        }

        /// <summary>Batch-mode entry for -executeMethod. Reads -iconStudioJob path and exits with a code.</summary>
        public static void RunFromCommandLine()
        {
            int code = 4;
            Debug.Log(LogPrefix + "entry realtimeSinceStartup=" + Time.realtimeSinceStartup.ToString("0.0", CultureInfo.InvariantCulture) + "s graphics=" + SystemInfo.graphicsDeviceType);
            try
            {
                var args = Environment.GetCommandLineArgs();
                string jobPath = null;
                for (int i = 0; i < args.Length - 1; i++)
                    if (args[i] == "-iconStudioJob") jobPath = args[i + 1];
                if (string.IsNullOrEmpty(jobPath) || !File.Exists(jobPath))
                {
                    Debug.LogError(LogPrefix + "missing -iconStudioJob <path>");
                    code = 3;
                }
                else if (SystemInfo.graphicsDeviceType == GraphicsDeviceType.Null)
                {
                    // -nographics: renders and read-backs return an uninitialised constant (0xCD), not an image.
                    Debug.LogError(LogPrefix + "no graphics device (-nographics?); rendering is impossible");
                    code = 5;
                }
                else
                {
                    var job = Json.Parse(File.ReadAllText(jobPath, Encoding.UTF8)) as Dictionary<string, object>;
                    if (job == null) { code = 3; Debug.LogError(LogPrefix + "job root is not an object"); }
                    else
                    {
                        // -executeMethod runs before the editor loop ticks, so callbacks that projects schedule
                        // with EditorApplication.delayCall on load (e.g. shader patchers) have not run yet.
                        if (Json.Bool(job, "flushDelayCalls", true))
                            Debug.Log(LogPrefix + FlushDelayCalls(Json.Int(job, "flushRounds", 40)));
                        RunJob(job, Path.GetDirectoryName(Path.GetFullPath(jobPath)), out var failed);
                        code = failed == 0 ? 0 : 2;
                    }
                }
            }
            catch (Exception e)
            {
                Debug.LogError(LogPrefix + "exception: " + e);
                code = 4;
            }
            Debug.Log(LogPrefix + "exit " + code);
            EditorApplication.Exit(code);
        }

        static string FlushDelayCalls(int rounds)
        {
            int calls = 0, r = 0;
            for (; r < rounds; r++)
            {
                var pending = EditorApplication.delayCall;
                if (pending == null) break;
                EditorApplication.delayCall = null;
                foreach (var d in pending.GetInvocationList())
                {
                    calls++;
                    try { ((EditorApplication.CallbackFunction)d)(); }
                    catch (Exception e) { Debug.LogWarning(LogPrefix + "delayCall " + d.Method.DeclaringType + "." + d.Method.Name + " threw " + e.Message); }
                }
            }
            return "flushed delayCall: " + calls + " call(s) in " + r + " round(s)" + (EditorApplication.delayCall != null ? ", still pending" : "");
        }

        // ------------------------------------------------------------------ job

        static string RunJob(Dictionary<string, object> job, string baseDir, out int failed)
        {
            var total = Stopwatch.StartNew();
            failed = 0;
            var defaults = Json.Obj(job, "defaults") ?? new Dictionary<string, object>();
            var outputDir = Resolve(baseDir, Json.Str(job, "outputDir", "."));
            Directory.CreateDirectory(outputDir);
            var resultPath = Resolve(baseDir, Json.Str(job, "resultPath", Path.Combine(outputDir, "result.json")));

            var sb = new StringBuilder();
            sb.Append("{\"schema\":\"").Append(Schema).Append("\",");
            sb.Append("\"unity\":").Append(Json.Quote(Application.unityVersion)).Append(',');
            sb.Append("\"batchMode\":").Append(Application.isBatchMode ? "true" : "false").Append(',');
            sb.Append("\"graphics\":").Append(Json.Quote(SystemInfo.graphicsDeviceType.ToString())).Append(',');
            sb.Append("\"colorSpace\":").Append(Json.Quote(QualitySettings.activeColorSpace.ToString())).Append(',');
            var rp = GraphicsSettings.currentRenderPipeline;
            sb.Append("\"renderPipeline\":").Append(Json.Quote(rp ? rp.GetType().Name : "Built-in")).Append(',');
            sb.Append("\"sceneDirtyBefore\":").Append(DirtyScenes()).Append(',');
            sb.Append("\"items\":[");

            var overrides = new PipelineOverrides();
            bool savedAsync = ShaderUtil.allowAsyncCompilation;
            Scene preview = default;
            try
            {
                ShaderUtil.allowAsyncCompilation = false; // avoid placeholder (cyan) shaders on the first render
                overrides.Apply(defaults);
                preview = EditorSceneManager.NewPreviewScene();
                var items = Json.Arr(job, "items") ?? new List<object>();
                Debug.Log(LogPrefix + "items " + items.Count);
                for (int i = 0; i < items.Count; i++)
                {
                    var item = items[i] as Dictionary<string, object>;
                    if (item == null) continue;
                    var opt = Merge(defaults, item);
                    var r = CaptureItem(preview, opt, outputDir, i);
                    if (!r.ok) failed++;
                    if (i > 0) sb.Append(',');
                    sb.Append(r.ToJson());
                    Debug.Log(LogPrefix + (r.ok ? "ok " : "FAIL ") + r.id + " " + r.ms + "ms " + (r.error ?? r.output));
                }
            }
            finally
            {
                if (preview.IsValid()) EditorSceneManager.ClosePreviewScene(preview);
                overrides.Restore();
                ShaderUtil.allowAsyncCompilation = savedAsync;
            }

            sb.Append("],");
            sb.Append("\"failed\":").Append(failed).Append(',');
            sb.Append("\"sceneDirtyAfter\":").Append(DirtyScenes()).Append(',');
            sb.Append("\"overrides\":").Append(Json.Quote(overrides.Describe())).Append(',');
            sb.Append("\"totalMs\":").Append(total.ElapsedMilliseconds).Append(',');
            sb.Append("\"ok\":").Append(failed == 0 ? "true" : "false").Append('}');
            var text = sb.ToString();
            Directory.CreateDirectory(Path.GetDirectoryName(resultPath));
            File.WriteAllText(resultPath, text, new UTF8Encoding(false));
            return text;
        }

        // ------------------------------------------------------------------ one item

        class ItemResult
        {
            public string id, prefab, output, error;
            public bool ok;
            public long ms, renderMs;
            public Bounds bounds;
            public int renderers, skipped, width, height;
            public float coverage, partialAlpha;
            public RectInt opaqueRect;
            public string notes = "";

            public string ToJson()
            {
                var sb = new StringBuilder("{");
                sb.Append("\"id\":").Append(Json.Quote(id)).Append(',');
                sb.Append("\"prefab\":").Append(Json.Quote(prefab)).Append(',');
                sb.Append("\"ok\":").Append(ok ? "true" : "false").Append(',');
                if (error != null) sb.Append("\"error\":").Append(Json.Quote(error)).Append(',');
                if (output != null) sb.Append("\"output\":").Append(Json.Quote(output)).Append(',');
                sb.Append("\"size\":[").Append(width).Append(',').Append(height).Append("],");
                sb.Append("\"bounds\":{\"center\":").Append(V(bounds.center)).Append(",\"size\":").Append(V(bounds.size)).Append("},");
                sb.Append("\"renderers\":").Append(renderers).Append(',');
                sb.Append("\"skippedRenderers\":").Append(skipped).Append(',');
                sb.Append("\"coverage\":").Append(F(coverage)).Append(',');
                sb.Append("\"partialAlpha\":").Append(F(partialAlpha)).Append(',');
                sb.Append("\"opaqueRect\":[").Append(opaqueRect.x).Append(',').Append(opaqueRect.y).Append(',').Append(opaqueRect.width).Append(',').Append(opaqueRect.height).Append("],");
                sb.Append("\"notes\":").Append(Json.Quote(notes.Trim())).Append(',');
                sb.Append("\"renderMs\":").Append(renderMs).Append(',');
                sb.Append("\"ms\":").Append(ms).Append('}');
                return sb.ToString();
            }
        }

        static ItemResult CaptureItem(Scene preview, Dictionary<string, object> o, string outputDir, int index)
        {
            var sw = Stopwatch.StartNew();
            var r = new ItemResult();
            r.prefab = Json.Str(o, "prefab", "");
            r.id = Json.Str(o, "id", Path.GetFileNameWithoutExtension(r.prefab));
            GameObject instance = null;
            var temp = new List<Object>();
            try
            {
                var prefab = AssetDatabase.LoadAssetAtPath<GameObject>(r.prefab);
                if (prefab == null) throw new CaptureError("prefab_not_found");
                if (prefab.GetComponentInChildren<RectTransform>(true) != null && !Json.Bool(o, "allowUi", false))
                    throw new CaptureError("ui_prefab");

                instance = (GameObject)PrefabUtility.InstantiatePrefab(prefab, preview);
                if (instance == null) throw new CaptureError("instantiate_failed");
                // "rotation": euler degrees for the root, e.g. for parts authored in a bone's space.
                var rotArr = Json.Arr(o, "rotation");
                var rootRot = rotArr != null && rotArr.Count == 3
                    ? Quaternion.Euler(Convert.ToSingle(rotArr[0], CultureInfo.InvariantCulture), Convert.ToSingle(rotArr[1], CultureInfo.InvariantCulture), Convert.ToSingle(rotArr[2], CultureInfo.InvariantCulture))
                    : Quaternion.identity;
                instance.transform.SetPositionAndRotation(Vector3.zero, rootRot);

                // Pose: "asIs" keeps the prefab's stored pose; "defaultState" samples the default state of the
                // first Animator layer at poseTime (seconds) without entering play mode.
                var pose = Json.Str(o, "pose", "asIs");
                if (pose == "defaultState") r.notes += SampleDefaultPose(instance, Json.Num(o, "poseTime", 0f)) + " ";

                var renderers = CollectRenderers(instance, o, r);
                if (renderers.Count == 0) throw new CaptureError("no_renderers");
                var bounds = renderers[0].bounds;
                foreach (var rd in renderers) bounds.Encapsulate(rd.bounds);
                r.bounds = bounds;
                r.renderers = renderers.Count;

                int size = Json.Int(o, "size", 1024);
                int ss = Mathf.Clamp(Json.Int(o, "supersample", 1), 1, 4);
                r.width = r.height = size;

                // Camera
                var camGo = new GameObject("IconStudioCamera");
                SceneManager.MoveGameObjectToScene(camGo, preview);
                temp.Add(camGo);
                var cam = camGo.AddComponent<Camera>();
                cam.scene = preview;
                cam.enabled = false;
                cam.cameraType = CameraType.Game;
                cam.allowMSAA = Json.Bool(o, "msaa", false);
                cam.allowHDR = Json.Bool(o, "hdr", true);
                cam.clearFlags = CameraClearFlags.SolidColor;
                ConfigurePipelineCamera(camGo, o);

                PlaceCamera(cam, bounds, o, renderers);
                PlaceLights(preview, cam, o, temp);

                var bg = Json.Str(o, "background", "transparent");
                var w = size * ss;
                var renderMethod = Json.Str(o, "renderMethod", "auto");
                var swRender = Stopwatch.StartNew();
                Texture2D tex;
                var lighting = Json.Bool(o, "overrideLighting", true);
                if (lighting) { Unsupported.SetOverrideLightingSettings(preview); ApplyAmbient(o); }
                try
                {
                    int warm = Json.Int(o, "warmupFrames", 1);
                    for (int i = 0; i < warm; i++) { var t0 = Render(cam, w, ColorOf(o, "backgroundColor", Color.clear), o, renderMethod, ref r.notes); Object.DestroyImmediate(t0); }

                    // "fit": "silhouette" re-frames from a small test render so the visible pixels (not the
                    // bounding box) fill 1 - 2 * padding of the canvas and sit in the middle.
                    if (Json.Str(o, "fit", "silhouette") == "silhouette")
                        r.notes += FitSilhouette(cam, o, renderMethod, ref r.notes) + " ";

                    if (bg == "matte")
                    {
                        var black = Render(cam, w, new Color(0, 0, 0, 1), o, renderMethod, ref r.notes);
                        var white = Render(cam, w, new Color(1, 1, 1, 1), o, renderMethod, ref r.notes);
                        tex = Matte(black, white);
                        Object.DestroyImmediate(black); Object.DestroyImmediate(white);
                    }
                    else if (bg == "solid")
                    {
                        tex = Render(cam, w, ColorOf(o, "backgroundColor", Color.white), o, renderMethod, ref r.notes);
                    }
                    else
                    {
                        tex = Render(cam, w, new Color(0, 0, 0, 0), o, renderMethod, ref r.notes);
                    }
                }
                finally
                {
                    if (lighting) Unsupported.RestoreOverrideLightingSettings();
                }
                r.renderMs = swRender.ElapsedMilliseconds;

                if (ss > 1) { var d = Downsample(tex, ss); Object.DestroyImmediate(tex); tex = d; }
                Stats(tex, r);
                if (r.coverage <= 0f) r.notes += "empty_image ";
                if (IsUniform(tex)) { Object.DestroyImmediate(tex); throw new CaptureError("uniform_image"); }

                var outName = Json.Str(o, "output", r.id + ".png");
                var outPath = Path.IsPathRooted(outName) ? outName : Path.Combine(outputDir, outName);
                Directory.CreateDirectory(Path.GetDirectoryName(outPath));
                File.WriteAllBytes(outPath, tex.EncodeToPNG());
                Object.DestroyImmediate(tex);
                r.output = outPath.Replace('\\', '/');
                r.ok = r.coverage > 0f || Json.Bool(o, "allowEmpty", false);
                if (!r.ok) r.error = "empty_image";
            }
            catch (CaptureError e) { r.error = e.Message; }
            catch (Exception e) { r.error = "exception: " + e.GetType().Name + ": " + e.Message; }
            finally
            {
                foreach (var t in temp) if (t) Object.DestroyImmediate(t);
                if (instance) Object.DestroyImmediate(instance);
                r.ms = sw.ElapsedMilliseconds;
            }
            return r;
        }

        class CaptureError : Exception { public CaptureError(string m) : base(m) { } }

        // ------------------------------------------------------------------ renderers and bounds

        static List<Renderer> CollectRenderers(GameObject root, Dictionary<string, object> o, ItemResult r)
        {
            bool hideParticles = Json.Bool(o, "hideParticles", true);
            bool bakeSkinned = Json.Bool(o, "bakeSkinnedBounds", true);
            var hideNames = (Json.Arr(o, "hideObjectsNamed") ?? new List<object>()).Select(x => x.ToString().ToLowerInvariant()).ToList();

            // LOD groups: keep LOD0 only.
            var lodSkip = new HashSet<Renderer>();
            foreach (var lg in root.GetComponentsInChildren<LODGroup>(true))
            {
                var lods = lg.GetLODs();
                for (int i = 1; i < lods.Length; i++) foreach (var rd in lods[i].renderers) if (rd) lodSkip.Add(rd);
            }

            var list = new List<Renderer>();
            foreach (var rd in root.GetComponentsInChildren<Renderer>(true))
            {
                bool particle = rd is ParticleSystemRenderer || rd is TrailRenderer || rd is LineRenderer || rd is BillboardRenderer;
                bool named = hideNames.Count > 0 && hideNames.Any(n => rd.name.ToLowerInvariant().Contains(n));
                if ((particle && hideParticles) || lodSkip.Contains(rd) || named)
                {
                    rd.enabled = false;
                    r.skipped++;
                    continue;
                }
                if (!rd.enabled || !rd.gameObject.activeInHierarchy || particle) continue;
                if (rd is SkinnedMeshRenderer smr && bakeSkinned && smr.sharedMesh != null)
                {
                    // Recompute bounds from the current pose; stored localBounds can be stale.
                    // Measured on 6000.6: BakeMesh(m, false) returns vertices with the lossy scale applied in the
                    // renderer's rotated space; BakeMesh(m, true) returned them unscaled. Bake with false, rotate and
                    // translate only, and keep the union with the stored bounds so a version difference can only
                    // over-frame, never clip.
                    var before = rd.bounds;
                    var m = new Mesh();
                    smr.BakeMesh(m, false);
                    var tr = smr.transform;
                    var b = TransformBounds(m.bounds, Matrix4x4.TRS(tr.position, tr.rotation, Vector3.one));
                    b.Encapsulate(before);
                    Object.DestroyImmediate(m);
                    // localBounds is expressed in the root bone's space when a root bone is set.
                    var space = smr.rootBone != null ? smr.rootBone : tr;
                    smr.localBounds = TransformBounds(b, space.worldToLocalMatrix);
                }
                list.Add(rd);
            }
            if (Json.Bool(o, "debugRenderers", false))
                r.notes += "renderers:" + string.Join("|", list.Select(x => x.name + "(" + x.GetType().Name + " " + F(x.bounds.size.magnitude) + ")")) + " ";
            return list;
        }

        static Bounds TransformBounds(Bounds b, Matrix4x4 m)
        {
            var c = b.center; var e = b.extents;
            var res = new Bounds(m.MultiplyPoint3x4(c), Vector3.zero);
            for (int i = 0; i < 8; i++)
            {
                var p = c + Vector3.Scale(e, new Vector3((i & 1) == 0 ? -1 : 1, (i & 2) == 0 ? -1 : 1, (i & 4) == 0 ? -1 : 1));
                res.Encapsulate(m.MultiplyPoint3x4(p));
            }
            return res;
        }

        static string SampleDefaultPose(GameObject root, float time)
        {
            var animator = root.GetComponentInChildren<Animator>(true);
            if (animator == null || animator.runtimeAnimatorController == null) return "pose:no_animator";
            var rac = animator.runtimeAnimatorController;
            var overrideCtrl = rac as AnimatorOverrideController;
            var ctrl = (overrideCtrl != null ? overrideCtrl.runtimeAnimatorController : rac) as UnityEditor.Animations.AnimatorController;
            AnimationClip clip = null;
            if (ctrl != null && ctrl.layers.Length > 0)
            {
                var st = ctrl.layers[0].stateMachine.defaultState;
                var motion = st != null ? st.motion : null;
                while (motion is UnityEditor.Animations.BlendTree bt && bt.children.Length > 0) motion = bt.children[0].motion;
                clip = motion as AnimationClip;
                if (clip != null && overrideCtrl != null && overrideCtrl[clip] != null) clip = overrideCtrl[clip];
            }
            if (clip == null) return "pose:no_default_clip(" + (rac != null ? rac.GetType().Name : "null") + ")";
            clip.SampleAnimation(animator.gameObject, Mathf.Clamp(time, 0f, clip.length));
            return "pose:" + clip.name + "@" + time.ToString("0.##", CultureInfo.InvariantCulture);
        }

        // ------------------------------------------------------------------ camera and light

        static Vector3 Axis(string s)
        {
            switch (s)
            {
                case "-z": return Vector3.back;
                case "+x": return Vector3.right;
                case "-x": return Vector3.left;
                default: return Vector3.forward;
            }
        }

        /// <summary>
        /// yaw: degrees the camera orbits from the model's front axis. Positive yaw shows the model's
        /// left-hand side face on the RIGHT of the image (camera moves to the viewer's right).
        /// pitch: degrees the camera looks down from horizontal. padding: empty margin per side (0..0.45).
        /// </summary>
        static void PlaceCamera(Camera cam, Bounds b, Dictionary<string, object> o, List<Renderer> renderers)
        {
            float yaw = Json.Num(o, "yaw", 28f), pitch = Json.Num(o, "pitch", 30f);
            var front = Axis(Json.Str(o, "front", "+z"));
            var right = Vector3.Cross(Vector3.up, front);
            float y = yaw * Mathf.Deg2Rad, p = pitch * Mathf.Deg2Rad;
            var horiz = Mathf.Cos(y) * front - Mathf.Sin(y) * right;
            var toCam = (Mathf.Cos(p) * horiz + Mathf.Sin(p) * Vector3.up).normalized;
            var rot = Quaternion.LookRotation(-toCam, Vector3.up);
            float radius = Mathf.Max(b.extents.magnitude, 0.01f);
            float padding = Mathf.Clamp(Json.Num(o, "padding", 0.06f), 0f, 0.45f);
            bool ortho = Json.Str(o, "projection", "orthographic") != "perspective";

            // Fit the projected bounding-box corners (tighter than the bounding sphere).
            var corners = new List<Vector3>();
            foreach (var rd in renderers)
            {
                var rb = rd.bounds;
                for (int i = 0; i < 8; i++)
                    corners.Add(rb.center + Vector3.Scale(rb.extents, new Vector3((i & 1) == 0 ? -1 : 1, (i & 2) == 0 ? -1 : 1, (i & 4) == 0 ? -1 : 1)));
            }
            var inv = Quaternion.Inverse(rot);
            float minX = float.MaxValue, maxX = float.MinValue, minY = float.MaxValue, maxY = float.MinValue;
            foreach (var c in corners)
            {
                var v = inv * (c - b.center);
                minX = Mathf.Min(minX, v.x); maxX = Mathf.Max(maxX, v.x);
                minY = Mathf.Min(minY, v.y); maxY = Mathf.Max(maxY, v.y);
            }
            var centerOffset = rot * new Vector3((minX + maxX) * 0.5f, (minY + maxY) * 0.5f, 0f);
            float half = Mathf.Max(maxX - minX, maxY - minY) * 0.5f / (1f - 2f * padding);
            var target = b.center + centerOffset;

            float dist = Json.Num(o, "cameraDistance", 0f);
            if (ortho)
            {
                cam.orthographic = true;
                cam.orthographicSize = half;
                if (dist <= 0f) dist = radius * 3f + 1f;
            }
            else
            {
                float fov = Json.Num(o, "fov", 20f);
                cam.orthographic = false;
                cam.fieldOfView = fov;
                if (dist <= 0f) dist = half / Mathf.Tan(fov * 0.5f * Mathf.Deg2Rad) + radius;
            }
            cam.transform.SetPositionAndRotation(target + toCam * dist, rot);
            cam.nearClipPlane = Mathf.Max(0.01f, dist - radius * 1.5f);
            cam.farClipPlane = dist + radius * 1.5f + 1f;
            cam.aspect = 1f;
        }

        static string FitSilhouette(Camera cam, Dictionary<string, object> o, string method, ref string notes)
        {
            const int probe = 256;
            float padding = Mathf.Clamp(Json.Num(o, "padding", 0.06f), 0f, 0.45f);
            var t = Render(cam, probe, new Color(0, 0, 0, 0), o, method, ref notes);
            var tmp = new ItemResult();
            Stats(t, tmp);
            Object.DestroyImmediate(t);
            if (tmp.coverage <= 0f) return "fit:empty";
            var rc = tmp.opaqueRect; // top-left origin
            float cx = (rc.x + rc.width * 0.5f) / probe - 0.5f;
            float cy = 0.5f - (rc.y + rc.height * 0.5f) / probe;
            float longSide = Mathf.Max(rc.width, rc.height) / (float)probe;
            float scale = longSide / (1f - 2f * padding);
            var tr = cam.transform;
            if (cam.orthographic)
            {
                float full = cam.orthographicSize * 2f;
                tr.position += tr.right * (cx * full) + tr.up * (cy * full);
                cam.orthographicSize *= scale;
            }
            else
            {
                // Rotate the view toward the silhouette centre, then narrow the field of view.
                float tanHalf = Mathf.Tan(cam.fieldOfView * 0.5f * Mathf.Deg2Rad);
                var dir = (tr.forward + tr.right * (cx * 2f * tanHalf) + tr.up * (cy * 2f * tanHalf)).normalized;
                tr.rotation = Quaternion.LookRotation(dir, Vector3.up);
                cam.fieldOfView = 2f * Mathf.Atan(tanHalf * scale) * Mathf.Rad2Deg;
            }
            return "fit:x" + scale.ToString("0.###", CultureInfo.InvariantCulture);
        }

        static void PlaceLights(Scene preview, Camera cam, Dictionary<string, object> o, List<Object> temp)
        {
            var lo = Json.Obj(o, "light") ?? new Dictionary<string, object>();
            float el = Json.Num(lo, "elevation", 50f) * Mathf.Deg2Rad;
            float az = Json.Num(lo, "azimuth", -30f) * Mathf.Deg2Rad; // relative to the camera, negative = from the left
            var back = -cam.transform.forward; back.y = 0; back = back.sqrMagnitude < 1e-6f ? Vector3.back : back.normalized;
            var rightH = Vector3.Cross(Vector3.up, back);   // camera's right on the ground plane
            rightH = -rightH;                               // Cross(up, back) points left; flip to camera right
            var from = Mathf.Cos(el) * (Mathf.Cos(az) * back + Mathf.Sin(az) * rightH) + Mathf.Sin(el) * Vector3.up;
            var go = new GameObject("IconStudioKeyLight");
            SceneManager.MoveGameObjectToScene(go, preview);
            temp.Add(go);
            var light = go.AddComponent<Light>();
            light.type = LightType.Directional;
            light.color = ColorOf(lo, "color", new Color(1f, 0.96f, 0.84f));
            light.intensity = Json.Num(lo, "intensity", 1f);
            light.shadows = Json.Bool(lo, "shadows", true) ? LightShadows.Soft : LightShadows.None;
            go.transform.rotation = Quaternion.LookRotation(-from.normalized, Vector3.up);
        }

        static void ApplyAmbient(Dictionary<string, object> o)
        {
            RenderSettings.ambientMode = AmbientMode.Flat;
            RenderSettings.ambientLight = ColorOf(o, "ambient", new Color(0.6f, 0.6f, 0.6f));
            var sh = new SphericalHarmonicsL2();
            sh.AddAmbientLight(RenderSettings.ambientLight);
            RenderSettings.ambientProbe = sh;
            RenderSettings.fog = false;
            RenderSettings.skybox = null;
        }

        static void ConfigurePipelineCamera(GameObject camGo, Dictionary<string, object> o)
        {
            // URP: UniversalAdditionalCameraData via reflection so this file compiles without URP.
            var t = Type.GetType("UnityEngine.Rendering.Universal.UniversalAdditionalCameraData, Unity.RenderPipelines.Universal.Runtime");
            if (t == null) return;
            var data = camGo.GetComponent(t) ?? camGo.AddComponent(t);
            Set(data, "renderPostProcessing", Json.Bool(o, "postProcessing", false));
            Set(data, "renderShadows", Json.Bool(o, "shadows", true));
            var aaType = t.GetProperty("antialiasing")?.PropertyType;
            if (aaType != null)
            {
                var aa = Json.Str(o, "antialiasing", "None");
                try { Set(data, "antialiasing", Enum.Parse(aaType, aa, true)); } catch { }
            }
            var rendererIndex = Json.Int(o, "rendererIndex", -1);
            if (rendererIndex >= 0) t.GetMethod("SetRenderer")?.Invoke(data, new object[] { rendererIndex });
        }

        static void Set(object target, string prop, object value)
        {
            var p = target.GetType().GetProperty(prop);
            if (p != null && p.CanWrite) p.SetValue(target, value);
        }

        // ------------------------------------------------------------------ render and read back

        static Texture2D Render(Camera cam, int size, Color bg, Dictionary<string, object> o, string method, ref string notes)
        {
            bool hdrTarget = Json.Bool(o, "hdrTarget", false);
            var desc = new RenderTextureDescriptor(size, size,
                hdrTarget ? RenderTextureFormat.ARGBHalf : RenderTextureFormat.ARGB32, 24)
            { sRGB = !hdrTarget, msaaSamples = 1 };
            var rt = RenderTexture.GetTemporary(desc);
            var prevActive = RenderTexture.active;
            try
            {
                cam.backgroundColor = bg;
                cam.targetTexture = rt;
                bool usedRequest = false;
#if UNITY_2023_1_OR_NEWER
                // Render requests arrived in 2023.1; older versions (and the Built-in pipeline) use Camera.Render.
                if (method == "request" || method == "auto")
                {
                    var req = new RenderPipeline.StandardRequest { destination = rt };
                    if (GraphicsSettings.currentRenderPipeline != null && RenderPipeline.SupportsRenderRequest(cam, req))
                    {
                        RenderPipeline.SubmitRenderRequest(cam, req);
                        usedRequest = true;
                    }
                    else if (method == "request") notes += "request_unsupported ";
                }
#else
                if (method == "request") notes += "request_unsupported ";
#endif
                if (!usedRequest) cam.Render();
                if (!notes.Contains(usedRequest ? "via:request" : "via:camera")) notes += usedRequest ? "via:request " : "via:camera ";

                RenderTexture.active = rt;
                var tex = new Texture2D(size, size, hdrTarget ? TextureFormat.RGBAFloat : TextureFormat.RGBA32, false, !hdrTarget ? false : true);
                tex.ReadPixels(new Rect(0, 0, size, size), 0, 0, false);
                tex.Apply(false, false);
                if (hdrTarget) tex = HdrToSrgb8(tex);
                return tex;
            }
            finally
            {
                cam.targetTexture = null;
                RenderTexture.active = prevActive;
                RenderTexture.ReleaseTemporary(rt);
            }
        }

        static Texture2D HdrToSrgb8(Texture2D src)
        {
            var px = src.GetPixels();
            var outTex = new Texture2D(src.width, src.height, TextureFormat.RGBA32, false, false);
            var o = new Color32[px.Length];
            for (int i = 0; i < px.Length; i++)
            {
                var c = px[i];
                o[i] = new Color32(
                    (byte)Mathf.RoundToInt(Mathf.Clamp01(Mathf.LinearToGammaSpace(Mathf.Clamp01(c.r))) * 255f),
                    (byte)Mathf.RoundToInt(Mathf.Clamp01(Mathf.LinearToGammaSpace(Mathf.Clamp01(c.g))) * 255f),
                    (byte)Mathf.RoundToInt(Mathf.Clamp01(Mathf.LinearToGammaSpace(Mathf.Clamp01(c.b))) * 255f),
                    (byte)Mathf.RoundToInt(Mathf.Clamp01(c.a) * 255f));
            }
            outTex.SetPixels32(o);
            outTex.Apply(false, false);
            Object.DestroyImmediate(src);
            return outTex;
        }

        /// <summary>Alpha from a black and a white render: a = 1 - (white - black); color = black / a.</summary>
        static Texture2D Matte(Texture2D black, Texture2D white)
        {
            var b = black.GetPixels32();
            var w = white.GetPixels32();
            var o = new Color32[b.Length];
            // Blending happens in linear space in a Linear project, so decode the sRGB bytes first.
            bool lin = QualitySettings.activeColorSpace == ColorSpace.Linear;
            var dec = new float[256];
            for (int i = 0; i < 256; i++) dec[i] = lin ? Mathf.GammaToLinearSpace(i / 255f) : i / 255f;
            for (int i = 0; i < b.Length; i++)
            {
                float br = dec[b[i].r], bgc = dec[b[i].g], bb = dec[b[i].b];
                float diff = ((dec[w[i].r] - br) + (dec[w[i].g] - bgc) + (dec[w[i].b] - bb)) / 3f;
                float a = Mathf.Clamp01(1f - diff);
                if (a < 1f / 255f) { o[i] = new Color32(0, 0, 0, 0); continue; }
                o[i] = new Color32(Enc(br / a, lin), Enc(bgc / a, lin), Enc(bb / a, lin), (byte)Mathf.RoundToInt(a * 255f));
            }
            var t = new Texture2D(black.width, black.height, TextureFormat.RGBA32, false, false);
            t.SetPixels32(o);
            t.Apply(false, false);
            return t;
        }

        static byte Enc(float v, bool lin)
        {
            v = Mathf.Clamp01(v);
            return (byte)Mathf.RoundToInt((lin ? Mathf.LinearToGammaSpace(v) : v) * 255f);
        }

        static Texture2D Downsample(Texture2D src, int f)
        {
            int w = src.width / f, h = src.height / f;
            var s = src.GetPixels32();
            var o = new Color32[w * h];
            for (int y = 0; y < h; y++)
                for (int x = 0; x < w; x++)
                {
                    float r = 0, g = 0, b = 0, a = 0;
                    for (int j = 0; j < f; j++)
                        for (int i = 0; i < f; i++)
                        {
                            var c = s[(y * f + j) * src.width + x * f + i];
                            float ca = c.a / 255f;
                            r += c.r * ca; g += c.g * ca; b += c.b * ca; a += ca; // premultiplied average
                        }
                    if (a <= 0f) { o[y * w + x] = new Color32(0, 0, 0, 0); continue; }
                    o[y * w + x] = new Color32((byte)Mathf.RoundToInt(r / a), (byte)Mathf.RoundToInt(g / a), (byte)Mathf.RoundToInt(b / a), (byte)Mathf.RoundToInt(a / (f * f) * 255f));
                }
            var t = new Texture2D(w, h, TextureFormat.RGBA32, false, false);
            t.SetPixels32(o);
            t.Apply(false, false);
            return t;
        }

        /// <summary>True when every pixel has the same value (no device, or nothing was drawn on an opaque clear).</summary>
        static bool IsUniform(Texture2D tex)
        {
            var px = tex.GetPixels32();
            var first = px[0];
            for (int i = 1; i < px.Length; i++)
                if (px[i].r != first.r || px[i].g != first.g || px[i].b != first.b || px[i].a != first.a) return false;
            return first.a != 0; // fully transparent is reported as empty_image instead
        }

        static void Stats(Texture2D tex, ItemResult r)
        {
            var px = tex.GetPixels32();
            int w = tex.width, h = tex.height, any = 0, partial = 0;
            int x0 = w, y0 = h, x1 = -1, y1 = -1;
            for (int y = 0; y < h; y++)
                for (int x = 0; x < w; x++)
                {
                    var a = px[y * w + x].a;
                    if (a == 0) continue;
                    any++;
                    if (a < 255) partial++;
                    if (x < x0) x0 = x; if (x > x1) x1 = x; if (y < y0) y0 = y; if (y > y1) y1 = y;
                }
            r.coverage = any / (float)(w * h);
            r.partialAlpha = any > 0 ? partial / (float)any : 0f;
            // opaqueRect in image coordinates with the origin at the top-left.
            r.opaqueRect = any > 0 ? new RectInt(x0, h - 1 - y1, x1 - x0 + 1, y1 - y0 + 1) : new RectInt(0, 0, 0, 0);
        }

        // ------------------------------------------------------------------ pipeline overrides (memory only)

        class PipelineOverrides
        {
            readonly List<Action> restore = new List<Action>();
            readonly List<string> applied = new List<string>();

            public void Apply(Dictionary<string, object> defaults)
            {
                var rp = GraphicsSettings.currentRenderPipeline;
                if (rp == null) return;

                // Post-process alpha output (URP 17+): serialized private field, flipped in memory only.
                if (Json.Has(defaults, "allowPostProcessAlpha"))
                {
                    var f = rp.GetType().GetField("m_AllowPostProcessAlphaOutput", BindingFlags.Instance | BindingFlags.NonPublic);
                    if (f != null)
                    {
                        var old = f.GetValue(rp);
                        f.SetValue(rp, Json.Bool(defaults, "allowPostProcessAlpha", false));
                        restore.Add(() => f.SetValue(rp, old));
                        applied.Add("allowPostProcessAlpha=" + Json.Bool(defaults, "allowPostProcessAlpha", false));
                    }
                    else applied.Add("allowPostProcessAlpha:unsupported");
                }

                // Renderer features to switch off, matched by type-name substring (e.g. an outline feature).
                var names = (Json.Arr(defaults, "disableRendererFeatures") ?? new List<object>()).Select(x => x.ToString()).ToList();
                if (names.Count > 0)
                {
                    var listField = rp.GetType().GetField("m_RendererDataList", BindingFlags.Instance | BindingFlags.NonPublic);
                    var datas = listField?.GetValue(rp) as Array;
                    if (datas != null)
                        foreach (var d in datas)
                        {
                            if (d == null) continue;
                            var featuresProp = d.GetType().GetProperty("rendererFeatures");
                            var features = featuresProp?.GetValue(d) as System.Collections.IEnumerable;
                            if (features == null) continue;
                            foreach (var feat in features)
                            {
                                if (feat == null) continue;
                                var so = feat as ScriptableObject;
                                var typeName = feat.GetType().Name;
                                if (!names.Any(n => typeName.IndexOf(n, StringComparison.OrdinalIgnoreCase) >= 0)) continue;
                                var activeProp = feat.GetType().GetProperty("isActive");
                                var setActive = feat.GetType().GetMethod("SetActive");
                                if (activeProp == null || setActive == null) continue;
                                var was = (bool)activeProp.GetValue(feat);
                                setActive.Invoke(feat, new object[] { false });
                                restore.Add(() => setActive.Invoke(feat, new object[] { was }));
                                applied.Add("feature_off:" + typeName);
                                // SetActive marks nothing dirty on disk; we restore before returning.
                            }
                        }
                }
            }

            public void Restore()
            {
                for (int i = restore.Count - 1; i >= 0; i--) { try { restore[i](); } catch (Exception e) { Debug.LogWarning(LogPrefix + "restore failed: " + e.Message); } }
                restore.Clear();
            }

            public string Describe() => string.Join(", ", applied);
        }

        // ------------------------------------------------------------------ helpers

        static string DirtyScenes()
        {
            var sb = new StringBuilder("[");
            for (int i = 0; i < SceneManager.sceneCount; i++)
            {
                var s = SceneManager.GetSceneAt(i);
                if (i > 0) sb.Append(',');
                sb.Append("{\"path\":").Append(Json.Quote(s.path)).Append(",\"dirty\":").Append(s.isDirty ? "true" : "false").Append('}');
            }
            return sb.Append(']').ToString();
        }

        static Dictionary<string, object> Merge(Dictionary<string, object> a, Dictionary<string, object> b)
        {
            var d = new Dictionary<string, object>(a);
            foreach (var kv in b) d[kv.Key] = kv.Value;
            return d;
        }

        static string Resolve(string baseDir, string p) => Path.IsPathRooted(p) ? p : Path.GetFullPath(Path.Combine(baseDir, p));

        static Color ColorOf(Dictionary<string, object> o, string key, Color fallback)
        {
            var arr = Json.Arr(o, key);
            if (arr == null || arr.Count < 3) return fallback;
            return new Color(Convert.ToSingle(arr[0], CultureInfo.InvariantCulture), Convert.ToSingle(arr[1], CultureInfo.InvariantCulture),
                Convert.ToSingle(arr[2], CultureInfo.InvariantCulture), arr.Count > 3 ? Convert.ToSingle(arr[3], CultureInfo.InvariantCulture) : 1f);
        }

        static string V(Vector3 v) => "[" + F(v.x) + "," + F(v.y) + "," + F(v.z) + "]";
        static string F(float f) => f.ToString("0.####", CultureInfo.InvariantCulture);

        // ------------------------------------------------------------------ minimal JSON (no package dependency)

        static class Json
        {
            public static object Parse(string s) { int i = 0; var v = Value(s, ref i); return v; }

            public static bool Has(Dictionary<string, object> o, string k) => o != null && o.ContainsKey(k);
            public static Dictionary<string, object> Obj(Dictionary<string, object> o, string k) => o != null && o.TryGetValue(k, out var v) ? v as Dictionary<string, object> : null;
            public static List<object> Arr(Dictionary<string, object> o, string k) => o != null && o.TryGetValue(k, out var v) ? v as List<object> : null;
            public static string Str(Dictionary<string, object> o, string k, string d) => o != null && o.TryGetValue(k, out var v) && v != null ? v.ToString() : d;
            public static float Num(Dictionary<string, object> o, string k, float d) => o != null && o.TryGetValue(k, out var v) && v is double x ? (float)x : d;
            public static int Int(Dictionary<string, object> o, string k, int d) => o != null && o.TryGetValue(k, out var v) && v is double x ? (int)Math.Round(x) : d;
            public static bool Bool(Dictionary<string, object> o, string k, bool d) => o != null && o.TryGetValue(k, out var v) && v is bool x ? x : d;

            public static string Quote(string s)
            {
                if (s == null) return "null";
                var sb = new StringBuilder("\"");
                foreach (var c in s)
                {
                    switch (c)
                    {
                        case '"': sb.Append("\\\""); break;
                        case '\\': sb.Append("\\\\"); break;
                        case '\n': sb.Append("\\n"); break;
                        case '\r': sb.Append("\\r"); break;
                        case '\t': sb.Append("\\t"); break;
                        default:
                            if (c < 0x20) sb.Append("\\u").Append(((int)c).ToString("x4")); else sb.Append(c);
                            break;
                    }
                }
                return sb.Append('"').ToString();
            }

            static void Ws(string s, ref int i) { while (i < s.Length && char.IsWhiteSpace(s[i])) i++; }

            static object Value(string s, ref int i)
            {
                Ws(s, ref i);
                if (i >= s.Length) throw new FormatException("unexpected end");
                char c = s[i];
                if (c == '{')
                {
                    var d = new Dictionary<string, object>(); i++;
                    Ws(s, ref i);
                    if (s[i] == '}') { i++; return d; }
                    while (true)
                    {
                        Ws(s, ref i);
                        var k = Str(s, ref i);
                        Ws(s, ref i); if (s[i] != ':') throw new FormatException("':' expected at " + i); i++;
                        d[k] = Value(s, ref i);
                        Ws(s, ref i);
                        if (s[i] == ',') { i++; continue; }
                        if (s[i] == '}') { i++; return d; }
                        throw new FormatException("',' or '}' expected at " + i);
                    }
                }
                if (c == '[')
                {
                    var l = new List<object>(); i++;
                    Ws(s, ref i);
                    if (s[i] == ']') { i++; return l; }
                    while (true)
                    {
                        l.Add(Value(s, ref i));
                        Ws(s, ref i);
                        if (s[i] == ',') { i++; continue; }
                        if (s[i] == ']') { i++; return l; }
                        throw new FormatException("',' or ']' expected at " + i);
                    }
                }
                if (c == '"') return Str(s, ref i);
                if (s.Substring(i).StartsWith("true")) { i += 4; return true; }
                if (s.Substring(i).StartsWith("false")) { i += 5; return false; }
                if (s.Substring(i).StartsWith("null")) { i += 4; return null; }
                int st = i;
                while (i < s.Length && "+-0123456789.eE".IndexOf(s[i]) >= 0) i++;
                return double.Parse(s.Substring(st, i - st), CultureInfo.InvariantCulture);
            }

            static string Str(string s, ref int i)
            {
                if (s[i] != '"') throw new FormatException("string expected at " + i);
                i++;
                var sb = new StringBuilder();
                while (s[i] != '"')
                {
                    if (s[i] == '\\')
                    {
                        i++;
                        char e = s[i];
                        if (e == 'n') sb.Append('\n'); else if (e == 't') sb.Append('\t'); else if (e == 'r') sb.Append('\r');
                        else if (e == 'b') sb.Append('\b'); else if (e == 'f') sb.Append('\f');
                        else if (e == 'u') { sb.Append((char)Convert.ToInt32(s.Substring(i + 1, 4), 16)); i += 4; }
                        else sb.Append(e);
                        i++;
                    }
                    else sb.Append(s[i++]);
                }
                i++;
                return sb.ToString();
            }
        }
    }
}
#endif

/* Example job (unity-icon-studio.capture/1). Paths are relative to the job file unless rooted.
{
  "outputDir": "out",
  "resultPath": "out/result.json",
  "defaults": {
    "size": 1024, "supersample": 1,
    "projection": "orthographic", "fov": 20,
    "yaw": 28, "pitch": 30, "front": "+z", "padding": 0.06,
    "background": "transparent",            // transparent | solid | matte
    "backgroundColor": [1, 1, 1, 1],        // for solid
    "postProcessing": false, "hdr": true, "msaa": false, "antialiasing": "None",
    "renderMethod": "auto",                 // auto | request | camera
    "light": { "elevation": 50, "azimuth": -30, "intensity": 1, "color": [1, 0.96, 0.84], "shadows": true },
    "ambient": [0.6, 0.6, 0.6],
    "hideParticles": true, "pose": "asIs", "warmupFrames": 1,
    "disableRendererFeatures": [],          // type-name substrings, memory only
    "allowPostProcessAlpha": true           // only when postProcessing is true, memory only
  },
  "items": [
    { "id": "house", "prefab": "Assets/Prefabs/House.prefab", "output": "house.png" },
    { "id": "hat", "prefab": "Assets/Prefabs/Hat.prefab", "yaw": 25, "pitch": 18 }
  ]
}
*/

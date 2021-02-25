
#version 130

uniform sampler2D tex_unit;
in  mediump vec2 ex_tex_coord;
out mediump vec4 out_frag_color;

void main(void) {
     out_frag_color = texture(tex_unit, ex_tex_coord);
}

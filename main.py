height = 1000
width = 1000
max_iter = 1000
for y in range(height):
    cy = (y - height/2.0) * 4.0 / height
    for x in range(width):
        zx = 0.0
        zy = 0.0
        cx = (x - width/2.0) * 4.0 / width
        i = 0
        while zx*zx + zy*zy < 4.0 and i < max_iter:
            tmp = zx*zx - zy*zy + cx
            zy = 2.0*zx*zy + cy
            zx = tmp
            i = i + 1
        if i == max_iter:
            print(str(cx) + " " + str(cy))

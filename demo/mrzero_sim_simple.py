from pathlib import Path

import numpy as np
import matplotlib.pyplot as plt

import MRzeroCore as mr0

from mrzero_sim import build_simdata

HERE = Path(__file__).parent
PHANTOM_RES = (128, 128, 1)

seqfn = str(HERE / "data" / "tse.seq")


## what i wanted
#data = build_simdata(HERE / "data" / "subj42-3T.json", combine=True)


##what seems to be better, but also doesnt work...
data = build_simdata(HERE / "data" / "subj42-3T.json", combine=True)
#


seq = mr0.Sequence.import_file(seqfn)
graph = mr0.compute_graph(seq, data)
signal = mr0.execute_graph(graph, seq, data)
kspace = seq.get_kspace()

recon = mr0.reco_adjoint(signal, kspace, resolution=PHANTOM_RES)

plt.subplot(1, 2, 1)
mr0.util.imshow(np.abs(recon))
plt.subplot(1, 2, 2)
mr0.util.imshow(np.angle(recon))
plt.show()
